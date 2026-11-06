//! 「按应用配置」表：扩展页签的一种，一行固定的默认行加上若干「应用 + 文本」行，存在扩展的数据集合中。
//!
//! 记录字段为 `appId`、`appName`、`text`；默认行的 key 为 [`DEFAULT_ROW`]。扩展用 `sayso.store.list` 读取。

use serde::Serialize;

use crate::protocol::AppInfo;
use crate::store::StoreRecord;

pub const DEFAULT_ROW: &str = "default";
/// 表中还没有行时显示的空行，编辑后才保存。
const FIRST_ROW: &str = "first";

const APP_ID: &str = "appId";
const APP_NAME: &str = "appName";
const TEXT: &str = "text";

#[derive(Clone, Debug, PartialEq, Serialize, uniffi::Record)]
pub struct AppRow {
    pub key: String,
    /// 还没选应用时为 None。
    pub app: Option<AppInfo>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AppTable {
    pub default_text: String,
    /// 按添加顺序排列，至少一行。
    pub rows: Vec<AppRow>,
    /// 可选的应用：正在运行的应用，加上已选但已退出的应用，按名称排序。
    pub apps: Vec<AppInfo>,
}

impl AppTable {
    pub(crate) fn new(mut records: Vec<StoreRecord>, running: Vec<AppInfo>) -> Self {
        records.sort_by(|lhs, rhs| lhs.created_at.cmp(&rhs.created_at));
        let default_text = records.iter().find(|record| record.key == DEFAULT_ROW).map_or_else(String::new, text);
        let mut rows: Vec<AppRow> = records
            .iter()
            .filter(|record| record.key != DEFAULT_ROW)
            .map(|record| AppRow { key: record.key.clone(), app: app(record), text: text(record) })
            .collect();
        if rows.is_empty() {
            rows.push(AppRow { key: FIRST_ROW.into(), app: None, text: String::new() });
        }

        let mut apps: Vec<AppInfo> =
            running.into_iter().chain(rows.iter().filter_map(|row| row.app.clone())).filter(|app| app.id.is_some()).collect();
        apps.sort_by_key(|app| (app.name.clone().unwrap_or_default().to_lowercase(), app.id.clone()));
        apps.dedup_by(|lhs, rhs| lhs.id == rhs.id);
        AppTable { default_text, rows, apps }
    }

    /// 只显示、尚未保存的行：加新行前要先保存，否则它会被新行取代。
    pub(crate) fn unsaved_rows(self) -> Vec<String> {
        self.rows.into_iter().map(|row| row.key).filter(|key| key == FIRST_ROW).collect()
    }
}

fn app(record: &StoreRecord) -> Option<AppInfo> {
    let field = |name| record.fields.get(name).and_then(|value: &serde_json::Value| value.as_str()).map(str::to_owned);
    field(APP_ID).map(|id| AppInfo { id: Some(id), name: field(APP_NAME) })
}

fn text(record: &StoreRecord) -> String {
    record.fields.get(TEXT).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned()
}

/// 一行要保存的字段，文本截断到长度上限。
pub(crate) fn fields(app: Option<AppInfo>, text: &str, max_length: usize) -> serde_json::Map<String, serde_json::Value> {
    let (id, name) = app.map_or((None, None), |app| (app.id, app.name));
    serde_json::Map::from_iter([
        (APP_ID.to_owned(), id.into()),
        (APP_NAME.to_owned(), name.into()),
        (TEXT.to_owned(), text.chars().take(max_length).collect::<String>().into()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(key: &str, created_at: &str, app: Option<AppInfo>, text: &str) -> StoreRecord {
        StoreRecord {
            key: key.into(),
            created_at: created_at.into(),
            updated_at: created_at.into(),
            fields: fields(app, text, 400),
        }
    }

    fn app(id: &str, name: &str) -> AppInfo {
        AppInfo { id: Some(id.into()), name: Some(name.into()) }
    }

    #[test]
    fn an_empty_table_shows_one_unsaved_row() {
        let table = AppTable::new(vec![record(DEFAULT_ROW, "1", None, "base")], vec![]);
        assert_eq!(table.default_text, "base");
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.unsaved_rows(), [FIRST_ROW]);
    }

    #[test]
    fn apps_keep_selected_apps_that_quit_without_duplicates() {
        let records =
            vec![record("b", "2", Some(app("id.xcode", "Xcode")), "x"), record("a", "1", Some(app("id.slack", "Slack")), "s")];
        let table = AppTable::new(records, vec![app("id.xcode", "Xcode"), AppInfo { id: None, name: Some("x".into()) }]);
        assert_eq!(table.rows.iter().map(|row| row.key.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(table.apps, [app("id.slack", "Slack"), app("id.xcode", "Xcode")]);
    }

    #[test]
    fn saved_text_is_cut_to_the_limit() {
        assert_eq!(fields(None, "abcdef", 3)[TEXT], "abc");
    }
}
