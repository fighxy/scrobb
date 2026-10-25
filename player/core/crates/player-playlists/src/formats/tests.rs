//! Образцы — в том виде, как их пишут реальные программы: foobar2000, AIMP,
//! VLC, ListenBrainz, Exportify, Excel, списки для Яндекс Музыки.

use super::*;
use crate::{ItemId, PlaylistId};
use std::collections::BTreeMap;

/// Windows-1251 для кириллицы и ASCII — так пишет M3U старый AIMP.
fn cp1251(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| match c {
            'Ё' => 0xA8,
            'ё' => 0xB8,
            'А'..='я' => (c as u32 - 0x410 + 0xC0) as u8,
            c if c.is_ascii() => c as u8,
            _ => panic!("нет в тесте: {c}"),
        })
        .collect()
}

fn one(imported: &Imported) -> &Entry {
    assert_eq!(imported.entries.len(), 1, "{imported:?}");
    &imported.entries[0]
}

#[test]
fn foobar2000_m3u8_with_bom_crlf_and_relative_windows_paths() {
    let text = "\u{FEFF}#EXTM3U\r\n#PLAYLIST:Вечер\r\n#EXTINF:251,Кино - Кукушка\r\n..\\Музыка\\Кино\\04 Кукушка.flac\r\n\r\n#EXTINF:-1,\r\nhttp://radio.example/stream\r\n";
    let got = parse(
        Format::M3u,
        text.as_bytes(),
        Some("C:\\Users\\Иван\\Плейлисты"),
    )
    .unwrap();
    assert_eq!(got.title.as_deref(), Some("Вечер"));
    let first = &got.entries[0];
    assert_eq!(
        first.location.as_deref(),
        Some("C:\\Users\\Иван\\Музыка\\Кино\\04 Кукушка.flac")
    );
    assert_eq!(first.track.artists, ["Кино"]);
    assert_eq!(first.track.title, "Кукушка");
    assert_eq!(first.track.duration_ms, Some(251_000));
    let radio = &got.entries[1];
    assert_eq!(
        radio.location.as_deref(),
        Some("http://radio.example/stream")
    );
    assert_eq!(radio.track.duration_ms, None);
    assert_eq!(radio.track.title, "stream");
}

#[test]
fn aimp_m3u_in_cp1251_without_extinf() {
    let bytes = cp1251(
        "D:\\Музыка\\Кино\\Группа крови.mp3\n#EXTALB:Группа крови\n#EXTART:Кино\nЁлка - Прованс.mp3\n",
    );
    let got = parse(detect("Мой.m3u", &bytes), &bytes, Some("D:\\Музыка")).unwrap();
    assert_eq!(
        got.entries[0].location.as_deref(),
        Some("D:\\Музыка\\Кино\\Группа крови.mp3")
    );
    assert_eq!(got.entries[0].track.title, "Группа крови");
    assert!(got.entries[0].track.artists.is_empty());
    let second = &got.entries[1];
    assert_eq!(
        second.location.as_deref(),
        Some("D:\\Музыка\\Ёлка - Прованс.mp3")
    );
    assert_eq!(
        second.track.title, "Ёлка - Прованс",
        "без #EXTINF имя файла — название"
    );
    assert_eq!(
        second.track.artists,
        ["Кино"],
        "артиста больше неоткуда взять"
    );
    assert_eq!(
        second.track.album_artist.as_deref(),
        Some("Кино"),
        "#EXTART — артист альбома"
    );
    assert_eq!(second.track.album.as_deref(), Some("Группа крови"));
}

#[test]
fn album_directives_apply_to_every_following_entry() {
    let text = "#EXTM3U\n#EXTALB:Группа крови\n#EXTART:Кино\n01 Группа крови.flac\n#EXTINF:286,Виктор Цой - Закрой за мной дверь\n02.flac\n";
    let got = parse(Format::M3u, text.as_bytes(), Some("/music")).unwrap();
    let [first, second] = &got.entries[..] else {
        panic!("две записи");
    };
    assert_eq!(first.track.album.as_deref(), Some("Группа крови"));
    assert_eq!(
        first.track.artists,
        ["Кино"],
        "артиста больше неоткуда взять"
    );
    assert_eq!(second.track.album.as_deref(), Some("Группа крови"));
    assert_eq!(second.track.album_artist.as_deref(), Some("Кино"));
    assert_eq!(second.track.artists, ["Виктор Цой"], "#EXTINF важнее");
}

#[test]
fn vlc_xspf_with_file_uris_entities_and_extensions() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist xmlns="http://xspf.org/ns/0/" xmlns:vlc="http://www.videolan.org/vlc/playlist/ns/0/" version="1">
	<title>Плейлист</title>
	<trackList>
		<track>
			<location>file:///home/ivan/%D0%9C%D1%83%D0%B7%D1%8B%D0%BA%D0%B0/Simon%20&amp;%20Garfunkel%20-%20The%20Boxer.flac</location>
			<title>The Boxer</title>
			<creator>Simon &amp; Garfunkel</creator>
			<album>Bridge over Troubled Water</album>
			<duration>308000</duration>
			<identifier>https://musicbrainz.org/recording/0E3A42E8-1CE5-4A0B-9F7D-11B7F9F0D10F</identifier>
			<extension application="http://www.videolan.org/vlc/playlist/0">
				<vlc:id>0</vlc:id>
			</extension>
		</track>
	</trackList>
	<extension application="http://www.videolan.org/vlc/playlist/0">
		<vlc:item tid="0"/>
	</extension>
</playlist>"#;
    let got = parse(detect("vlc.xspf", xml.as_bytes()), xml.as_bytes(), None).unwrap();
    assert_eq!(got.title.as_deref(), Some("Плейлист"));
    let e = one(&got);
    assert_eq!(
        e.location.as_deref(),
        Some("/home/ivan/Музыка/Simon & Garfunkel - The Boxer.flac")
    );
    assert_eq!(e.track.artists, ["Simon & Garfunkel"]);
    assert_eq!(e.track.album.as_deref(), Some("Bridge over Troubled Water"));
    assert_eq!(e.track.duration_ms, Some(308_000));
    assert_eq!(
        e.track.recording_mbid.as_deref(),
        Some("0e3a42e8-1ce5-4a0b-9f7d-11b7f9f0d10f")
    );
}

#[test]
fn listenbrainz_jspf_with_musicbrainz_extensions() {
    let json = r#"{
  "playlist": {
    "title": "1980s flashback jams",
    "creator": "rob",
    "identifier": "https://listenbrainz.org/playlist/a2b1c3d4-0000-4000-8000-000000000000",
    "extension": {
      "https://musicbrainz.org/doc/jspf#playlist": { "public": true, "creator": "rob" }
    },
    "track": [
      {
        "title": "Ward Off the Evil Spirits",
        "creator": "Ultravox",
        "album": "Rage in Eden",
        "identifier": ["https://musicbrainz.org/recording/e8f9b188-f819-4e43-ab0f-4bd26ce9ff56"],
        "extension": {
          "https://musicbrainz.org/doc/jspf#track": {
            "added_at": "2020-11-27T09:23:39Z",
            "artist_identifiers": ["https://musicbrainz.org/artist/f8c1a0c9-0000-4000-8000-000000000000"]
          }
        }
      },
      {
        "title": "Spotify-only",
        "creator": "Someone",
        "identifier": "https://musicbrainz.org/recording/11111111-2222-4333-8444-555555555555",
        "location": ["https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"],
        "duration": "201000"
      }
    ]
  }
}"#;
    let got = parse(detect("lb.json", json.as_bytes()), json.as_bytes(), None).unwrap();
    assert_eq!(got.title.as_deref(), Some("1980s flashback jams"));
    let first = &got.entries[0].track;
    assert_eq!(
        first.recording_mbid.as_deref(),
        Some("e8f9b188-f819-4e43-ab0f-4bd26ce9ff56")
    );
    assert_eq!(first.artists, ["Ultravox"]);
    let second = &got.entries[1];
    assert_eq!(
        second.track.recording_mbid.as_deref(),
        Some("11111111-2222-4333-8444-555555555555"),
        "identifier строкой"
    );
    assert_eq!(
        second.location.as_deref(),
        Some("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC")
    );
    assert_eq!(second.track.duration_ms, Some(201_000));
    assert!(parse(Format::Jspf, b"[1, 2]", None).is_err());
}

#[test]
fn exportify_csv_with_quotes_and_isrc() {
    let csv = "\"Track URI\",\"Track Name\",\"Artist Name(s)\",\"Album Name\",\"Album Artist Name(s)\",\"Track Duration (ms)\",\"ISRC\"\n\
               \"spotify:track:3n3Ppam7vgaVa1iaRUc9Lp\",\"September\",\"Earth, Wind & Fire\",\"The Best of Earth, Wind & Fire, Vol. 1\",\"Earth, Wind & Fire\",\"215133\",\"USSM17800845\"\n\
               \"spotify:track:x\",\"Say \"\"Hi\"\"\nagain\",\"A\",\"\",\"\",\"1000\",\"us-sm1-78-00846\"\n";
    let got = parse(detect("export.csv", csv.as_bytes()), csv.as_bytes(), None).unwrap();
    assert_eq!(got.entries.len(), 2);
    let t = &got.entries[0].track;
    assert_eq!(t.title, "September");
    assert_eq!(
        t.artists,
        ["Earth, Wind & Fire"],
        "запятая внутри кавычек — не разделитель"
    );
    assert_eq!(
        t.album.as_deref(),
        Some("The Best of Earth, Wind & Fire, Vol. 1")
    );
    assert_eq!(t.duration_ms, Some(215_133));
    assert_eq!(t.isrc(), Some("USSM17800845"));
    assert_eq!(
        t.ext_ids.get("spotify").map(String::as_str),
        Some("3n3Ppam7vgaVa1iaRUc9Lp")
    );
    let second = &got.entries[1].track;
    assert_eq!(second.title, "Say \"Hi\"\nagain");
    assert_eq!(second.isrc(), Some("USSM17800846"));
}

#[test]
fn exportify_net_columns_with_isrc_first() {
    // Нынешний exportify.net: ISRC вторым, «Duration (ms)», много лишних колонок.
    let csv = "Track URI,ISRC,Track Name,Album Name,Artist Name(s),Release Date,Duration (ms),Popularity,Genres\n\
               spotify:track:abc,RUA018700012,Кукушка,Чёрный альбом,Кино,1990-01-12,397000,55,\"russian rock,post-punk\"\n";
    let got = parse(detect("Кино.csv", csv.as_bytes()), csv.as_bytes(), None).unwrap();
    let t = &got.entries[0].track;
    assert_eq!(
        (t.title.as_str(), t.artists.as_slice()),
        ("Кукушка", &["Кино".to_owned()][..])
    );
    assert_eq!(t.album.as_deref(), Some("Чёрный альбом"));
    assert_eq!(t.duration_ms, Some(397_000));
    assert_eq!(t.isrc(), Some("RUA018700012"));
}

#[test]
fn excel_csv_with_semicolons_and_russian_headers() {
    let csv = "Исполнитель;Название;Альбом;Длительность\r\nКино;Кукушка;Чёрный альбом;6:37\r\nСплин;Романс;;225\r\n";
    let got = parse(Format::Csv, csv.as_bytes(), None).unwrap();
    assert_eq!(got.entries[0].track.artists, ["Кино"]);
    assert_eq!(got.entries[0].track.title, "Кукушка");
    assert_eq!(got.entries[0].track.duration_ms, Some(397_000));
    assert_eq!(got.entries[1].track.album, None);
    assert_eq!(
        got.entries[1].track.duration_ms,
        Some(225_000),
        "число без «мс» — секунды"
    );
    // Без заголовка — «артист, название».
    let bare = parse(Format::Csv, "Кино,Кукушка\n".as_bytes(), None).unwrap();
    assert_eq!(
        (
            bare.entries[0].track.artists.clone(),
            bare.entries[0].track.title.clone()
        ),
        (vec!["Кино".to_owned()], "Кукушка".to_owned())
    );
}

#[test]
fn text_lists_with_numbers_and_dashes() {
    let text = "# Мой список\n1. Кино - Кукушка\n02) Сплин — Романс\nЗемфира – Искала\nПросто название\n\n12 стульев - Песня\n";
    let got = parse(detect("list.txt", text.as_bytes()), text.as_bytes(), None).unwrap();
    let names: Vec<(Vec<String>, String)> = got
        .entries
        .iter()
        .map(|e| (e.track.artists.clone(), e.track.title.clone()))
        .collect();
    assert_eq!(
        names,
        [
            (vec!["Кино".to_owned()], "Кукушка".to_owned()),
            (vec!["Сплин".to_owned()], "Романс".to_owned()),
            (vec!["Земфира".to_owned()], "Искала".to_owned()),
            (vec![], "Просто название".to_owned()),
            (vec!["12 стульев".to_owned()], "Песня".to_owned()),
        ]
    );
}

#[test]
fn pls_entries_by_number() {
    let pls = "[playlist]\nNumberOfEntries=2\nFile2=b.mp3\nTitle2=Кино - Звезда\nLength2=-1\nFile1=file:///m/a.flac\nTitle1=Aквариум - Город\nLength1=261\nVersion=2\n";
    let got = parse(
        detect("radio.pls", pls.as_bytes()),
        pls.as_bytes(),
        Some("/m"),
    )
    .unwrap();
    assert_eq!(got.entries[0].location.as_deref(), Some("/m/a.flac"));
    assert_eq!(got.entries[0].track.duration_ms, Some(261_000));
    assert_eq!(got.entries[1].location.as_deref(), Some("/m/b.mp3"));
    assert_eq!(got.entries[1].track.title, "Звезда");
}

#[test]
fn detects_by_extension_then_content() {
    assert_eq!(detect("a.M3U8", b""), Format::M3u);
    assert_eq!(detect("a.json", b"{\"playlist\":{}}"), Format::Jspf);
    assert_eq!(detect("a.txt", b"#EXTM3U\n"), Format::M3u);
    assert_eq!(detect("noext", b"[playlist]\n"), Format::Pls);
    assert_eq!(detect("noext", b"<?xml version"), Format::Xspf);
    assert_eq!(detect("noext", "Кино - Кукушка".as_bytes()), Format::Txt);
}

fn sample() -> (Playlist, Vec<PlaylistItem>) {
    let playlist = Playlist {
        id: PlaylistId("019a1b2c-0000-7000-8000-000000000001".into()),
        title: "Вечер & \"дорога\"".into(),
        description: Some("Для поездок <за город>".into()),
        origin: None,
        created_at_ms: 1_761_381_065_000,
        updated_at_ms: 1_761_381_065_000,
    };
    let mut ext_ids = BTreeMap::new();
    ext_ids.insert("isrc".into(), "RUA018700012".into());
    let items = vec![
        PlaylistItem {
            id: ItemId("i1".into()),
            position: "V".into(),
            added_at_ms: 1_761_381_065_000,
            location: Some("/home/ivan/Музыка/Кино/04 Кукушка.flac".into()),
            track: TrackSnapshot {
                title: "Кукушка".into(),
                artists: vec!["Кино".into()],
                album: Some("Чёрный альбом".into()),
                album_artist: Some("Кино".into()),
                duration_ms: Some(397_000),
                recording_mbid: Some("6f1c2f5b-1c5c-4d6b-9a3e-0a1b2c3d4e5f".into()),
                ext_ids,
            },
        },
        PlaylistItem {
            id: ItemId("i2".into()),
            position: "e".into(),
            added_at_ms: 1_761_381_065_000,
            location: None,
            track: TrackSnapshot {
                title: "Sweet, \"Sweet\" Song".into(),
                artists: vec!["A".into(), "B".into()],
                ..TrackSnapshot::default()
            },
        },
    ];
    (playlist, items)
}

#[test]
fn every_export_reads_back() {
    let (playlist, items) = sample();
    for format in Format::EXPORT {
        let out = write(format, &playlist, &items, Some("/home/ivan/Плейлисты")).unwrap();
        let back = parse(format, out.text.as_bytes(), Some("/home/ivan/Плейлисты")).unwrap();
        let first = &back.entries[0];
        assert_eq!(first.track.title, "Кукушка", "{format:?}");
        assert_eq!(first.track.artists, ["Кино"], "{format:?}");
        match format {
            Format::M3u => {
                assert_eq!(out.skipped, 1, "без файла в M3U8 не попадает");
                assert!(
                    out.text.contains("\n../Музыка/Кино/04 Кукушка.flac\n"),
                    "путь относительно файла плейлиста"
                );
                assert!(out.text.contains("#EXTINF:397,Кино - Кукушка\n"));
                assert_eq!(back.title.as_deref(), Some(playlist.title.as_str()));
            }
            Format::Txt => assert_eq!(back.entries[1].track.artists, ["A, B"]),
            _ => {
                assert_eq!(back.entries.len(), 2, "{format:?}");
                assert_eq!(
                    back.entries[1].track.title, "Sweet, \"Sweet\" Song",
                    "{format:?}"
                );
                assert_eq!(first.track.duration_ms, Some(397_000), "{format:?}");
                assert_eq!(
                    first.track.album.as_deref(),
                    Some("Чёрный альбом"),
                    "{format:?}"
                );
                assert_eq!(
                    first.track.recording_mbid.as_deref(),
                    Some("6f1c2f5b-1c5c-4d6b-9a3e-0a1b2c3d4e5f"),
                    "{format:?}"
                );
            }
        }
        if matches!(
            format,
            Format::Xspf | Format::Jspf | Format::M3u | Format::Csv
        ) {
            assert_eq!(
                first.location.as_deref(),
                Some("/home/ivan/Музыка/Кино/04 Кукушка.flac"),
                "{format:?}"
            );
        }
        if matches!(format, Format::Xspf | Format::Jspf) {
            assert_eq!(
                back.title.as_deref(),
                Some(playlist.title.as_str()),
                "{format:?}"
            );
            assert_eq!(
                back.description.as_deref(),
                Some("Для поездок <за город>"),
                "{format:?}"
            );
        }
        if matches!(format, Format::Jspf | Format::Csv) {
            assert_eq!(first.track.isrc(), Some("RUA018700012"), "{format:?}");
        }
    }
    let jspf = write(Format::Jspf, &playlist, &items, None).unwrap();
    let back = parse(Format::Jspf, jspf.text.as_bytes(), None).unwrap();
    assert_eq!(
        back.entries[1].track.artists,
        ["A", "B"],
        "все артисты — в расширении Scrobb"
    );
    assert_eq!(back.entries[0].track.album_artist.as_deref(), Some("Кино"));
    assert!(jspf.text.contains("\"identifier\": [\n          \"https://musicbrainz.org/recording/6f1c2f5b-1c5c-4d6b-9a3e-0a1b2c3d4e5f\""));
    assert!(write(Format::Pls, &playlist, &items, None).is_err());
}
