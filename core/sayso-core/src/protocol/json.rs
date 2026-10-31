use serde::{Deserialize, Serialize};

/// 任意 JSON 值：扩展的回调载荷、数据记录字段与页签模板。经 UniFFI 传递时为 JSON 文本。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Json(pub serde_json::Value);

impl From<Json> for String {
    fn from(value: Json) -> String {
        value.0.to_string()
    }
}

impl TryFrom<String> for Json {
    type Error = serde_json::Error;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        serde_json::from_str(&text).map(Json)
    }
}

uniffi::custom_type!(Json, String);

/// 模板替换与搜索时使用的文本形式；整数不带小数点，null 为空串。
pub fn text_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 1e15 => {
                format!("{}", float as i64)
            }
            _ => number.to_string(),
        },
        other => other.to_string(),
    }
}

/// 递归替换所有字符串中的 `{{字段名}}` 占位符，只做文本替换。
pub fn substitute_placeholders(value: &serde_json::Value, resolve: &dyn Fn(&str) -> String) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => serde_json::Value::String(substitute(text, resolve)),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(|item| substitute_placeholders(item, resolve)).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields.iter().map(|(name, item)| (name.clone(), substitute_placeholders(item, resolve))).collect(),
        ),
        other => other.clone(),
    }
}

fn substitute(text: &str, resolve: &dyn Fn(&str) -> String) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}") else {
            break;
        };
        result.push_str(&rest[..open]);
        result.push_str(&resolve(rest[open + 2..open + 2 + close].trim()));
        rest = &rest[open + 2 + close + 2..];
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitutes_nested_placeholders_and_formats_numbers() {
        let template = json!({"title": "{{word}} ×{{ count }}", "list": ["{{missing}}!"]});
        let fields = json!({"word": "apple", "count": 3.0});
        let resolved = substitute_placeholders(&template, &|name| fields.get(name).map(text_value).unwrap_or_default());
        assert_eq!(resolved, json!({"title": "apple ×3", "list": ["!"]}));
    }
}
