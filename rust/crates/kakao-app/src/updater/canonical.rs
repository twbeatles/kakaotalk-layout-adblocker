use serde_json::Value;

use super::error::UpdateError;

pub fn canonical_payload(payload: &Value) -> Result<Vec<u8>, UpdateError> {
    serde_json::to_vec(payload).map_err(|err| UpdateError::Message(err.to_string()))
}

/// Match Python json.dumps(..., ensure_ascii=False, sort_keys=True, separators=(",", ":"))
pub fn canonical_payload_python(payload: &Value) -> Result<Vec<u8>, UpdateError> {
    let dumped = pythonish_dumps(payload);
    Ok(dumped.into_bytes())
}

fn pythonish_dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", escape_json_string(s)),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(pythonish_dumps).collect();
            format!("[{}]", inner.join(","))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let inner: Vec<String> = keys
                .into_iter()
                .map(|k| format!("\"{}\":{}", escape_json_string(k), pythonish_dumps(&map[k])))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
    }
}

fn escape_json_string(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_payload_sorts_keys() {
        let payload = json!({"b": 1, "a": "카카오"});
        assert_eq!(
            String::from_utf8(canonical_payload_python(&payload).unwrap()).unwrap(),
            "{\"a\":\"카카오\",\"b\":1}"
        );
    }
}
