//! `MemoryPlaylists` проходит общие проверки хранилища (`player-testkit`).

#[test]
fn memory_store_conformance() {
    player_testkit::playlists::conformance(player_playlists::MemoryPlaylists::new);
}
