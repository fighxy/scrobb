//! Ярус Scrobb для веба (заготовка).
//!
//! Только крейты, которые собираются под `wasm32-unknown-unknown`: DTO API,
//! правило скроббла, sans-IO клиент и синхронизация. Плеер сюда не входит.
//!
//! План: `wasm-bindgen` + `wasm-pack` → npm-пакет, который используют
//! веб-приложение Scrobb (Next.js) и расширение браузера. Так правило скроббла
//! и протокол живут в одном месте, а TypeScript-версии сверяются с теми же
//! векторами из `contracts/`.

pub use scrobb_api as api;
pub use scrobb_client as client;
pub use scrobb_rules as rules;
pub use scrobb_sync as sync;
