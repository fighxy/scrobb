import { useApp } from "@/app-context";
import { AlbumCard, ArtistCard, CardGrid } from "@/components/Cards";
import { Cover, InlineLink, PlayButton } from "@/components/common";
import { Eyebrow, Page, PageTitle } from "@/components/Page";
import { TrackTable } from "@/components/TrackTable";
import { ALBUMS, count, longDuration, TRACKS } from "@/format";
import { artistKey } from "@/lib/library";
import { useCoverColor } from "@/usePlayer";

function Gone({ what }: { what: string }) {
  return (
    <Page header={<PageTitle text={`${what} больше нет`} />}>
      <p className="px-6 text-muted-foreground">Возможно, папку убрали из библиотеки или файлы переместили.</p>
    </Page>
  );
}

/** Все альбомы плитками. */
export function AlbumsView() {
  const { lib } = useApp();
  return (
    <Page
      header={
        <>
          <Eyebrow>Библиотека</Eyebrow>
          <PageTitle text="Альбомы" />
          <div className="text-sm text-foreground/80">{count(lib.albums.length, ALBUMS)}</div>
        </>
      }
    >
      {lib.albums.length ? (
        <CardGrid items={lib.albums} render={(a) => <AlbumCard key={a.key} album={a} />} />
      ) : (
        <p className="px-6 py-8 text-muted-foreground">Альбомы появятся, когда у треков в тегах указан альбом.</p>
      )}
    </Page>
  );
}

/** Альбом: большая обложка, шапка цвета обложки, треки по порядку. */
export function AlbumView({ albumKey }: { albumKey: string }) {
  const app = useApp();
  const album = app.lib.albumByKey.get(albumKey);
  const tint = useCoverColor(app.backend, album?.cover ?? null);
  if (!album) return <Gone what="Альбома" />;
  const active = app.isActive(album.tracks);
  return (
    <Page
      tint={tint}
      header={
        <div className="flex flex-col gap-6 @xl:flex-row @xl:items-end">
          <Cover src={app.backend.coverUrl(album.cover, "l")} className="size-44 rounded-lg shadow-2xl shadow-black/50 @3xl:size-56" />
          <div className="min-w-0">
            <Eyebrow>Альбом</Eyebrow>
            <PageTitle text={album.title} />
            <div className="flex flex-wrap items-center gap-x-1.5 text-sm text-foreground/80">
              {album.artist && (
                <InlineLink className="font-semibold text-foreground" onClick={() => app.go({ kind: "artist", key: artistKey(album.artist) })}>
                  {album.artist}
                </InlineLink>
              )}
              <span>· {count(album.tracks.length, TRACKS)}</span>
              {album.durationMs > 0 && <span>· {longDuration(album.durationMs)}</span>}
            </div>
          </div>
        </div>
      }
    >
      <div className="px-6 py-5">
        <PlayButton
          playing={active && app.state.status === "playing"}
          onClick={() => (active ? app.toggle() : app.play(album.tracks, 0))}
        />
      </div>
      <TrackTable tracks={album.tracks} albumMode />
    </Page>
  );
}

/** Все артисты круглыми плитками. */
export function ArtistsView() {
  const { lib } = useApp();
  return (
    <Page
      header={
        <>
          <Eyebrow>Библиотека</Eyebrow>
          <PageTitle text="Артисты" />
          <div className="text-sm text-foreground/80">{lib.artists.length.toLocaleString("ru-RU")}</div>
        </>
      }
    >
      {lib.artists.length ? (
        <CardGrid items={lib.artists} minWidth={160} render={(a) => <ArtistCard key={a.key} artist={a} />} />
      ) : (
        <p className="px-6 py-8 text-muted-foreground">Артисты появятся вместе с треками.</p>
      )}
    </Page>
  );
}

/** Артист: его альбомы и все треки с его участием. */
export function ArtistView({ artistKey: key }: { artistKey: string }) {
  const app = useApp();
  const artist = app.lib.artistByKey.get(key);
  const tint = useCoverColor(app.backend, artist?.cover ?? null);
  if (!artist) return <Gone what="Артиста" />;
  const active = app.isActive(artist.tracks);
  return (
    <Page
      tint={tint}
      header={
        <div className="flex flex-col gap-6 @xl:flex-row @xl:items-end">
          <Cover round src={app.backend.coverUrl(artist.cover, "l")} className="size-44 shadow-2xl shadow-black/50 @3xl:size-56" />
          <div className="min-w-0">
            <Eyebrow>Артист</Eyebrow>
            <PageTitle text={artist.name} />
            <div className="text-sm text-foreground/80">
              {[artist.albums.length ? count(artist.albums.length, ALBUMS) : null, count(artist.tracks.length, TRACKS)]
                .filter(Boolean)
                .join(" · ")}
            </div>
          </div>
        </div>
      }
    >
      <div className="px-6 py-5">
        <PlayButton
          playing={active && app.state.status === "playing"}
          onClick={() => (active ? app.toggle() : app.play(artist.tracks, 0))}
        />
      </div>
      {artist.albums.length > 0 && (
        <section aria-label="Альбомы" className="mb-6">
          <h2 className="px-6 pb-1 text-xl font-bold">Альбомы</h2>
          <CardGrid items={artist.albums} render={(a) => <AlbumCard key={a.key} album={a} />} />
        </section>
      )}
      <section aria-label="Треки">
        <h2 className="px-6 pb-2 text-xl font-bold">Треки</h2>
        <TrackTable tracks={artist.tracks} />
      </section>
    </Page>
  );
}
