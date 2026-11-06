//! 扩展数据：每个扩展一个目录，每个集合一个 `<collection>.json`（记录数组），由 Sayso 托管。
//!
//! 所有写入经同一把锁串行执行并原子落盘；扩展只能通过 `sayso.store` 写入。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::time_util::now_iso;

const RESERVED_FIELDS: [&str; 3] = ["key", "createdAt", "updatedAt"];

/// 一条记录。`key`、`createdAt`、`updatedAt` 由 Sayso 维护，其余为扩展字段；JSON 为扁平对象。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreRecord {
    pub key: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

impl StoreRecord {
    /// 按字段名取值，包括三个保留字段。
    pub fn value(&self, field: &str) -> Option<serde_json::Value> {
        match field {
            "key" => Some(self.key.clone().into()),
            "createdAt" => Some(self.created_at.clone().into()),
            "updatedAt" => Some(self.updated_at.clone().into()),
            _ => self.fields.get(field).cloned(),
        }
    }
}

/// 一次数据写入。
#[derive(Clone, Debug, PartialEq)]
pub enum StoreWrite {
    /// 整条写入（覆盖扩展字段）。
    Put {
        collection: String,
        key: String,
        record: serde_json::Map<String, serde_json::Value>,
    },
    /// 局部更新，记录不存在时创建；`increment` 对数字字段做加法。
    Patch {
        collection: String,
        key: String,
        set: serde_json::Map<String, serde_json::Value>,
        increment: HashMap<String, f64>,
    },
    Delete {
        collection: String,
        key: String,
    },
}

impl StoreWrite {
    fn collection(&self) -> &str {
        match self {
            Self::Put { collection, .. } | Self::Patch { collection, .. } | Self::Delete { collection, .. } => {
                collection
            }
        }
    }
}

/// 集合名只允许字母、数字、下划线和连字符，避免路径穿越。
pub fn is_valid_collection_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 全部扩展的数据。缓存已加载的集合，写入时同步更新文件与缓存。
pub struct Stores {
    root: PathBuf,
    cache: Mutex<HashMap<(String, String), Vec<StoreRecord>>>,
}

impl Stores {
    pub fn new(root: PathBuf) -> Self {
        Stores { root, cache: Mutex::new(HashMap::new()) }
    }

    /// 某个扩展的数据目录，内置与外部扩展相同。
    pub fn directory(&self, extension_id: &str) -> PathBuf {
        self.root.join(extension_id)
    }

    pub fn records(&self, extension_id: &str, collection: &str) -> Result<Vec<StoreRecord>, CoreError> {
        let mut cache = self.cache.lock().unwrap();
        self.load(&mut cache, extension_id, collection).cloned()
    }

    pub fn apply(&self, extension_id: &str, write: StoreWrite) -> Result<(), CoreError> {
        let collection = write.collection().to_string();
        if !is_valid_collection_name(&collection) {
            return Err(CoreError::InvalidInstruction(format!("invalid collection name {collection}")));
        }
        let mut cache = self.cache.lock().unwrap();
        let mut records = self.load(&mut cache, extension_id, &collection)?.clone();
        let now = now_iso();
        match write {
            StoreWrite::Put { key, record, .. } => {
                let fields = without_reserved(record);
                match records.iter_mut().find(|item| item.key == key) {
                    Some(existing) => {
                        existing.fields = fields;
                        existing.updated_at = now;
                    }
                    None => records.push(StoreRecord { key, created_at: now.clone(), updated_at: now, fields }),
                }
            }
            StoreWrite::Patch { key, set, increment, .. } => {
                let index = match records.iter().position(|item| item.key == key) {
                    Some(index) => index,
                    None => {
                        records.push(StoreRecord {
                            key,
                            created_at: now.clone(),
                            updated_at: now.clone(),
                            fields: Default::default(),
                        });
                        records.len() - 1
                    }
                };
                let record = &mut records[index];
                record.fields.extend(without_reserved(set));
                for (name, delta) in increment.into_iter().filter(|(name, _)| !RESERVED_FIELDS.contains(&name.as_str()))
                {
                    let current = record.fields.get(&name).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
                    record.fields.insert(name, number(current + delta));
                }
                record.updated_at = now;
            }
            StoreWrite::Delete { key, .. } => records.retain(|item| item.key != key),
        }
        let directory = self.directory(extension_id);
        std::fs::create_dir_all(&directory)?;
        write_atomically(&file_path(&directory, &collection), &serde_json::to_vec_pretty(&records)?)?;
        cache.insert((extension_id.to_string(), collection), records);
        Ok(())
    }

    fn load<'a>(
        &self,
        cache: &'a mut HashMap<(String, String), Vec<StoreRecord>>,
        extension_id: &str,
        collection: &str,
    ) -> Result<&'a Vec<StoreRecord>, CoreError> {
        let key = (extension_id.to_string(), collection.to_string());
        if !cache.contains_key(&key) {
            let records = read(&self.directory(extension_id), collection)?;
            cache.insert(key.clone(), records);
        }
        Ok(&cache[&key])
    }
}

/// 读取某个集合；文件不存在视为空集合。
fn read(directory: &Path, collection: &str) -> Result<Vec<StoreRecord>, CoreError> {
    match std::fs::read(file_path(directory, collection)) {
        Ok(data) => Ok(serde_json::from_slice(&data)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(error) => Err(error.into()),
    }
}

fn file_path(directory: &Path, collection: &str) -> PathBuf {
    directory.join(format!("{collection}.json"))
}

/// 先写临时文件再改名，避免写到一半时崩溃留下损坏的数据。
pub(crate) fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, data)?;
    std::fs::rename(&temporary, path)
}

fn without_reserved(fields: serde_json::Map<String, serde_json::Value>) -> serde_json::Map<String, serde_json::Value> {
    fields.into_iter().filter(|(name, _)| !RESERVED_FIELDS.contains(&name.as_str())).collect()
}

/// 整数保持整数，与扩展写入时的 JSON 形式一致。
fn number(value: f64) -> serde_json::Value {
    if value.fract() == 0.0 && value.abs() < 9e15 {
        (value as i64).into()
    } else {
        serde_json::Number::from_f64(value).map_or(serde_json::Value::Null, serde_json::Value::Number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BlockContent, Json, Language, SortOrder, TabSort, TabSpec};
    use serde_json::json;

    fn map(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn patch_creates_increments_and_persists() {
        let root = tempfile::tempdir().unwrap();
        let stores = Stores::new(root.path().into());
        let patch = |set| StoreWrite::Patch {
            collection: "words".into(),
            key: "en|apple".into(),
            set: map(set),
            increment: HashMap::from([("count".to_string(), 1.0)]),
        };
        stores.apply("x", patch(json!({"term": "apple"}))).unwrap();
        // 保留字段不能被扩展覆盖。
        stores.apply("x", patch(json!({"key": "hack"}))).unwrap();

        let records = read(&stores.directory("x"), "words").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].key, "en|apple");
        assert_eq!(records[0].fields["count"], json!(2));
        assert_eq!(records[0].fields["term"], json!("apple"));
    }

    #[test]
    fn collection_name_cannot_escape_directory() {
        let root = tempfile::tempdir().unwrap();
        let stores = Stores::new(root.path().into());
        let write = StoreWrite::Put { collection: "../evil".into(), key: "k".into(), record: Default::default() };
        assert!(stores.apply("x", write).is_err());
    }

    #[test]
    fn tab_template_renders_sorts_and_searches() {
        let tab = TabSpec {
            title: "生词本".into(),
            icon: None,
            collection: "words".into(),
            item: Some(Json(
                json!({"type": "term", "term": "{{term}}", "gloss": "{{gloss}}", "detail": "{{example}}", "language": "{{language}}"}),
            )),
            app_table: None,
            search_fields: vec!["term".into(), "gloss".into()],
            sort: Some(TabSort { field: "term".into(), order: SortOrder::Desc }),
            item_actions: vec![Json(json!({"type": "delete"}))],
        };
        let record = |key: &str, term: &str, gloss: &str| StoreRecord {
            key: key.into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            fields: map(json!({"term": term, "gloss": gloss, "language": "en"})),
        };
        let records = vec![record("a", "apple", "苹果"), record("b", "banana", "香蕉")];

        let keys = |records: Vec<StoreRecord>| records.into_iter().map(|r| r.key).collect::<Vec<_>>();
        assert_eq!(keys(tab.arrange(records.clone(), "")), ["b", "a"]);
        assert_eq!(keys(tab.arrange(records.clone(), "苹")), ["a"]);

        let block = tab.render(&records[0]).unwrap();
        assert_eq!(block.id, "a");
        assert_eq!(
            block.content,
            BlockContent::Term {
                term: "apple".into(),
                gloss: Some("苹果".into()),
                language: Language::English,
                detail: Some("".into())
            }
        );
        assert_eq!(block.actions.len(), 1);
    }
}
