//! 扩展协议的数据结构。JSON 线上格式与 `docs/extension-protocol.md` 一致，同时经 UniFFI 提供给平台宿主。

mod block;
mod capability;
mod event;
mod json;
mod language;
mod manifest;

pub use block::*;
pub use capability::*;
pub use event::*;
pub use json::Json;
pub use language::*;
pub use manifest::*;

/// 扩展协议版本。删除字段或改变字段语义时提升；新增字段、事件、块类型不提升。
pub const SCHEMA_VERSION: u32 = 4;
/// 内置扩展独占的 id 前缀。
pub const BUILT_IN_PREFIX: &str = "sayso.";
/// Sayso 在扩展分组中代画的块（如词条卡片）独占的块 id 前缀，扩展不能用它新增或移除块。
pub const SAYSO_BLOCK_PREFIX: &str = "@";
