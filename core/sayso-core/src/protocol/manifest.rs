use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{json, Block, Capability, EventType, Json, BUILT_IN_PREFIX, SCHEMA_VERSION};
use crate::error::CoreError;
use crate::store::{is_valid_collection_name, StoreRecord};

/// 扩展清单 `extension.json`。内置扩展的清单随 core 编译嵌入，格式相同。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// 处理事件的 JS 模块，相对扩展目录。订阅了事件时必填。
    #[serde(default)]
    pub main: Option<String>,
    #[serde(default)]
    pub timeout_seconds: Option<u32>,
    /// 订阅的事件；只提供识别器的扩展可以省略。
    #[serde(default)]
    pub events: Vec<EventType>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub tab: Option<TabSpec>,
    /// 扩展提供的命令，由用户绑定到路由触发，以 `command` 事件交给本扩展。
    #[serde(default)]
    pub commands: Vec<CommandSpec>,
    /// 扩展提供的语音识别器，可在语音识别设置中选用。
    #[serde(default)]
    pub recognizer: Option<RecognizerSpec>,
}

/// 扩展命令。输入来源由用户为路由设置，命令本身只接收统一的 `input.text`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct CommandSpec {
    pub id: String,
    pub title: String,
}

/// 扩展提供的流式语音识别器：每次录音启动一次进程，录音期间持续收音频、回传识别文本。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct RecognizerSpec {
    /// 显示在语音识别设置中的名称。
    pub title: String,
    /// 识别器可执行文件及参数。首项是路径时相对扩展目录解析，否则在 PATH 中查找。
    pub command: Vec<String>,
    /// 录音结束后等待最终结果的最长时间，默认 30 秒。
    #[serde(default)]
    pub finish_timeout_seconds: Option<u32>,
}

impl RecognizerSpec {
    pub fn finish_timeout(&self) -> Duration {
        Duration::from_secs(self.finish_timeout_seconds.unwrap_or(30).clamp(1, 120).into())
    }
}

/// 扩展页签：按模板渲染扩展的某个数据集合。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct TabSpec {
    pub title: String,
    /// 图标名，平台各自映射到系统图标。
    #[serde(default)]
    pub icon: Option<String>,
    pub collection: String,
    /// 条目的块模板（不含 id，id 取记录的 key），字符串中可用 `{{字段名}}`。
    pub item: Json,
    #[serde(default)]
    pub search_fields: Vec<String>,
    #[serde(default)]
    pub sort: Option<TabSort>,
    /// 条目上的操作模板。
    #[serde(default)]
    pub item_actions: Vec<Json>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct TabSort {
    pub field: String,
    pub order: SortOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    Desc,
}

impl Manifest {
    pub fn is_built_in(&self) -> bool {
        self.id.starts_with(BUILT_IN_PREFIX)
    }

    pub fn grants(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_seconds.unwrap_or(10).clamp(1, 60).into())
    }

    pub fn validate(&self, built_in: bool) -> Result<(), CoreError> {
        let fail = |reason: String| Err(CoreError::InvalidManifest(reason));
        if self.schema_version != SCHEMA_VERSION {
            return fail(format!("unsupported schemaVersion {}", self.schema_version));
        }
        if self.id.is_empty() || self.name.is_empty() {
            return fail("id and name are required".into());
        }
        if self.is_built_in() != built_in {
            return fail(format!("id prefix {BUILT_IN_PREFIX} is reserved for built-in extensions"));
        }
        if self.events.is_empty() && self.recognizer.is_none() {
            return fail("no event subscribed and no recognizer provided".into());
        }
        if !self.events.is_empty() && self.main.as_deref().is_none_or(str::is_empty) {
            return fail("main is required when subscribing to events".into());
        }
        if let Some(recognizer) = &self.recognizer {
            if built_in {
                return fail("built-in extensions cannot provide recognizers".into());
            }
            if !self.grants(Capability::Audio) {
                return fail("recognizer requires the audio capability".into());
            }
            if recognizer.command.first().is_none_or(String::is_empty) || recognizer.title.is_empty() {
                return fail("recognizer command and title are required".into());
            }
        }
        if !self.commands.is_empty() {
            if !self.events.contains(&EventType::Command) {
                return fail("commands require the command event".into());
            }
            let mut ids: Vec<&str> = self.commands.iter().map(|command| command.id.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            if ids.len() != self.commands.len() || self.commands.iter().any(|c| c.id.is_empty() || c.title.is_empty()) {
                return fail("command ids must be unique and titles non-empty".into());
            }
        }
        if let Some(tab) = &self.tab {
            if !self.grants(Capability::Store) {
                return fail("tab requires the store capability".into());
            }
            if !is_valid_collection_name(&tab.collection) {
                return fail(format!("invalid collection name {}", tab.collection));
            }
        }
        Ok(())
    }
}

impl TabSpec {
    /// 用一条记录渲染页签条目。
    pub fn render(&self, record: &StoreRecord) -> Result<Block, CoreError> {
        let resolve = |name: &str| record.value(name).map(|value| json::text_value(&value)).unwrap_or_default();
        let serde_json::Value::Object(mut fields) = json::substitute_placeholders(&self.item.0, &resolve) else {
            return Err(CoreError::InvalidManifest("tab.item must be an object".into()));
        };
        fields.insert("id".into(), record.key.clone().into());
        let actions =
            self.item_actions.iter().map(|action| json::substitute_placeholders(&action.0, &resolve)).collect();
        fields.insert("actions".into(), serde_json::Value::Array(actions));
        serde_json::from_value(serde_json::Value::Object(fields))
            .map_err(|error| CoreError::InvalidManifest(error.to_string()))
    }

    /// 按页签配置搜索、排序记录。
    pub fn arrange(&self, records: Vec<StoreRecord>, query: &str) -> Vec<StoreRecord> {
        let query = query.trim().to_lowercase();
        let mut matched: Vec<StoreRecord> = records
            .into_iter()
            .filter(|record| {
                query.is_empty()
                    || self.search_fields.iter().any(|field| {
                        record
                            .value(field)
                            .is_some_and(|value| json::text_value(&value).to_lowercase().contains(&query))
                    })
            })
            .collect();
        if let Some(sort) = &self.sort {
            matched.sort_by(|lhs, rhs| {
                let ordering = compare(lhs.value(&sort.field), rhs.value(&sort.field));
                if sort.order == SortOrder::Asc {
                    ordering
                } else {
                    ordering.reverse()
                }
            });
        }
        matched
    }
}

/// 排序比较：数字按数值，其余按文本（ISO-8601 日期按文本即按时间）；缺失值排在最前。
fn compare(lhs: Option<serde_json::Value>, rhs: Option<serde_json::Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (lhs, rhs) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(serde_json::Value::Number(a)), Some(serde_json::Value::Number(b))) => {
            a.as_f64().partial_cmp(&b.as_f64()).unwrap_or(Ordering::Equal)
        }
        (Some(a), Some(b)) => json::text_value(&a).cmp(&json::text_value(&b)),
    }
}
