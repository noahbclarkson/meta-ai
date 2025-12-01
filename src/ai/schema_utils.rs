use serde_json::{json, Map, Value};
use serde::Serialize;

pub fn clean_schema<T: Serialize>(root: T) -> serde_json::Result<Value> {
    let mut root_val = serde_json::to_value(root)?;

    let definitions = root_val
        .get("definitions")
        .cloned()
        .or_else(|| root_val.get("$defs").cloned())
        .unwrap_or(json!({}))
        .as_object()
        .cloned()
        .unwrap_or_default();

    process_schema_node(&mut root_val, &definitions, 0);

    if let Value::Object(ref mut map) = root_val {
        map.remove("$schema");
        map.remove("title");
        map.remove("definitions");
        map.remove("$defs");
        map.remove("$id");
    }

    Ok(root_val)
}

fn process_schema_node(node: &mut Value, definitions: &Map<String, Value>, depth: usize) {
    // 0. Recursion Guard
    if depth > 20 {
        *node = json!({ "type": "object", "nullable": true });
        return;
    }

    // 1. Resolve $ref loop
    // We do this BEFORE matching on Value::Object to avoid holding a borrow on 'map'
    // while trying to assign to '*node'.
    let mut resolve_attempts = 0;
    loop {
        // Peek to see if we have a $ref
        let ref_target = if let Value::Object(map) = node {
            map.get("$ref").and_then(|v| v.as_str()).map(|s| s.to_string())
        } else {
            None
        };

        if let Some(def_name_full) = ref_target {
            resolve_attempts += 1;
            if resolve_attempts > 10 { 
                // Stop trying to resolve to prevent infinite loops
                break; 
            }

            let def_name = def_name_full.split('/').next_back().unwrap_or_default();
            if let Some(def) = definitions.get(def_name) {
                *node = def.clone();
                // Loop continues to check if the new node is also a ref
            } else {
                *node = json!({ "type": "object", "description": "Unresolvable reference" });
                break; 
            }
        } else {
            break; // No ref found, safe to proceed
        }
    }

    // 2. Handle Boolean Schemas (Gemini Fix)
    // schemars can generate `true` for "Any". Gemini Strict Mode requires a typed object.
    if let Value::Bool(allow_all) = node {
        if *allow_all {
            *node = json!({
                "type": ["string", "number", "boolean", "null"]
            });
        } else {
            *node = json!({ "not": {} }); 
        }
        // Node is now an object, proceed to cleanup below
    }

    // 3. Process Object/Array children
    match node {
        Value::Object(map) => {
            // Strict Mode Cleanup
            map.remove("$ref"); // Ensure removed if it lingered
            map.remove("additionalProperties");
            map.remove("$schema");
            map.remove("$id");
            map.remove("title");
            map.remove("default");
            map.remove("examples");

            // Fix "type" arrays
            if let Some(Value::Array(types)) = map.get("type") {
                if types.len() == 2 && types.contains(&json!("null")) {
                    if let Some(real_type) = types.iter().find(|t| *t != &json!("null")) {
                        let real_type_clone = real_type.clone();
                        map.insert("type".to_string(), real_type_clone);
                        map.insert("nullable".to_string(), json!(true));
                    }
                } else if !types.is_empty() {
                    let first = types[0].clone();
                    map.insert("type".to_string(), first);
                }
            }

            // Recurse into properties
            if let Some(Value::Object(props)) = map.get_mut("properties") {
                for val in props.values_mut() {
                    process_schema_node(val, definitions, depth + 1);
                }
            }
            
            // Recurse into items (for arrays)
            if let Some(val) = map.get_mut("items") {
                process_schema_node(val, definitions, depth + 1);
            }

            // Recurse into combinators
            for key in ["allOf", "anyOf", "oneOf"] {
                if let Some(Value::Array(arr)) = map.get_mut(key) {
                    for item in arr.iter_mut() {
                        process_schema_node(item, definitions, depth + 1);
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                process_schema_node(item, definitions, depth + 1);
            }
        }
        _ => {}
    }
}