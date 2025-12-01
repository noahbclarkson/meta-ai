use super::dsl::{CmpOp, LogicOp, AppProgram, ConstantValue, MathOp};
use crate::error::MetaError;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub data: Value,
}

impl RuntimeState {
    pub fn new(inputs: Value) -> Self {
        // Removed "outputs": {} to prevent fallback confusion
        Self {
            data: json!({
                "inputs": inputs,
                "temp": {}
            }),
        }
    }

    pub fn get(&self, path: &str) -> Result<Value, MetaError> {
        // 1. Try exact match
        if let Some(val) = self.data.pointer(path) {
            return Ok(val.clone());
        }

        // 2. Fallback: Check inside /inputs
        if path.starts_with('/') {
            let input_path = format!("/inputs{}", path);
            if let Some(val) = self.data.pointer(&input_path) {
                return Ok(val.clone());
            }
        }

        // 3. Failure - Generate Debug Info
        let available_roots = self.data.as_object()
            .map(|o| o.keys().cloned().collect::<Vec<String>>())
            .unwrap_or_default();
        
        let input_keys = self.data.pointer("/inputs").and_then(|v| v.as_object())
            .map(|o| o.keys().cloned().collect::<Vec<String>>());

        let hint = if let Some(keys) = input_keys {
            format!(" Available root keys: {:?}. Available input keys: {:?}", available_roots, keys)
        } else {
            format!(" Available root keys: {:?}", available_roots)
        };

        Err(MetaError::RuntimeError(format!("Pointer not found: '{}'.{}", path, hint)))
    }

    pub fn set(&mut self, path: &str, value: Value) -> Result<(), MetaError> {
        if let Some(target) = self.data.pointer_mut(path) {
            *target = value;
        } else {
            let parts: Vec<&str> = path.split('/').collect();
            
            // Handle /key (Root level)
            if parts.len() == 2 && !parts[1].is_empty() {
                let key = parts[1];
                if let Some(root) = self.data.as_object_mut() {
                    root.insert(key.to_string(), value);
                    return Ok(());
                }
            }
            
            // Handle /section/key (Standard)
            if parts.len() == 3 {
                let section = parts[1];
                let key = parts[2];
                
                if let Some(root) = self.data.as_object_mut() {
                    if !root.contains_key(section) {
                        root.insert(section.to_string(), json!({}));
                    }
                    if let Some(section_obj) = root.get_mut(section).and_then(|v| v.as_object_mut()) {
                        section_obj.insert(key.to_string(), value);
                        return Ok(());
                    }
                }
            }
            return Err(MetaError::RuntimeError(format!("Cannot set path (invalid structure): {path}")));
        }
        Ok(())
    }
}

pub struct Runtime;

impl Runtime {
    pub fn execute(program: &AppProgram, inputs: Value) -> Result<Value, MetaError> {
        let mut state = RuntimeState::new(inputs);
        
        log::info!("🚀 Executing Program: {}", program.definition.name);

        for step in &program.steps {
            log::debug!("   Step [{}]: {}", step.id, step.description);
            let result = Self::exec_op(&step.operation, &state)?;
            state.set(&step.output_path, result)?;
        }

        // --- NEW OUTPUT EXTRACTION LOGIC ---
        // Instead of returning state.data or looking for a magic "outputs" key,
        // we explicitly construct the output based on the Output Schema.
        if let Some(props) = program.definition.output_schema.get("properties").and_then(|v| v.as_object()) {
            let mut structured_output = Map::new();
            for key in props.keys() {
                // 1. Look in root (e.g., "total_profit")
                if let Some(val) = state.data.get(key) {
                    structured_output.insert(key.clone(), val.clone());
                } 
                // 2. Look in pointer path (e.g., "/total_profit") just in case
                else if let Some(val) = state.data.pointer(&format!("/{}", key)) {
                    structured_output.insert(key.clone(), val.clone());
                }
            }
            
            // If we found any matching data, return it.
            if !structured_output.is_empty() {
                return Ok(Value::Object(structured_output));
            }
        }

        // Fallback: If no schema properties matched (or schema is empty), return full state
        Ok(state.data)
    }

    fn exec_op(op: &LogicOp, state: &RuntimeState) -> Result<Value, MetaError> {
        match op {
            LogicOp::Get { path } => state.get(path),
            LogicOp::Constant { value } => {
                Ok(match value {
                    ConstantValue::String(s) => json!(s),
                    ConstantValue::Number(n) => json!(n),
                    ConstantValue::Bool(b) => json!(b),
                    ConstantValue::Null => Value::Null,
                })
            },
            LogicOp::Add { a, b } => Ok(json!(get_f64(state, a)? + get_f64(state, b)?)),
            LogicOp::Subtract { a, b } => Ok(json!(get_f64(state, a)? - get_f64(state, b)?)),
            LogicOp::Multiply { a, b } => Ok(json!(get_f64(state, a)? * get_f64(state, b)?)),
            LogicOp::Divide { a, b } => {
                let v2 = get_f64(state, b)?;
                if v2 == 0.0 { return Err(MetaError::RuntimeError("Division by zero".into())); }
                Ok(json!(get_f64(state, a)? / v2))
            },
            LogicOp::Calculate { list_path, output_field, operator, a_field, b_field } => {
                let mut arr = get_array(state, list_path)?;
                let resolve_operand = |obj: &Map<String, Value>, target: &str| -> f64 {
                    if target.starts_with('/') {
                        state.get(target).ok().and_then(|v| v.as_f64()).unwrap_or(0.0)
                    } else {
                        obj.get(target).and_then(|v| v.as_f64()).unwrap_or(0.0)
                    }
                };
                for item in &mut arr {
                    if let Some(obj) = item.as_object_mut() {
                        let v1 = resolve_operand(obj, a_field);
                        let v2 = resolve_operand(obj, b_field);
                        let res = match operator {
                            MathOp::Add => v1 + v2,
                            MathOp::Subtract => v1 - v2,
                            MathOp::Multiply => v1 * v2,
                            MathOp::Divide => if v2 != 0.0 { v1 / v2 } else { 0.0 },
                        };
                        obj.insert(output_field.clone(), json!(res));
                    }
                }
                Ok(json!(arr))
            },
            LogicOp::Sum { list_path, field } => {
                let arr = get_array(state, list_path)?;
                let sum: f64 = arr.iter().filter_map(|item| {
                    if let Some(f) = field { item.get(f).and_then(|v| v.as_f64()) }
                    else { item.as_f64() }
                }).sum();
                Ok(json!(sum))
            },
            LogicOp::Count { list_path } => {
                let arr = get_array(state, list_path)?;
                Ok(json!(arr.len()))
            },
            LogicOp::Min { list_path, field } => {
                let arr = get_array(state, list_path)?;
                let val = arr.iter().filter_map(|item| {
                    if let Some(f) = field { item.get(f).and_then(|v| v.as_f64()) }
                    else { item.as_f64() }
                }).fold(f64::INFINITY, f64::min);
                Ok(json!(val))
            },
            LogicOp::Max { list_path, field } => {
                let arr = get_array(state, list_path)?;
                let val = arr.iter().filter_map(|item| {
                    if let Some(f) = field { item.get(f).and_then(|v| v.as_f64()) }
                    else { item.as_f64() }
                }).fold(f64::NEG_INFINITY, f64::max);
                Ok(json!(val))
            },
            LogicOp::Pluck { path, key } => {
                let arr = get_array(state, path)?;
                let plucked: Vec<Value> = arr.iter()
                    .map(|obj| obj.get(key).cloned().unwrap_or(Value::Null))
                    .collect();
                Ok(json!(plucked))
            },
            LogicOp::Sort { list_path, field, descending } => {
                let mut arr = get_array(state, list_path)?;
                arr.sort_by(|a, b| {
                    let val_a = a.get(field).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let val_b = b.get(field).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    val_a.partial_cmp(&val_b).unwrap_or(std::cmp::Ordering::Equal)
                });
                if *descending { arr.reverse(); }
                Ok(json!(arr))
            },
            LogicOp::FilterNumeric { list_path, field, operator, value } => {
                let arr = get_array(state, list_path)?;
                let filtered: Vec<Value> = arr.into_iter().filter(|item| {
                    let val = if let Some(f) = field { item.get(f).and_then(|v| v.as_f64()) }
                              else { item.as_f64() };
                    if let Some(v) = val {
                        match operator {
                            CmpOp::Gt => v > *value,
                            CmpOp::Lt => v < *value,
                            CmpOp::Eq => (v - *value).abs() < f64::EPSILON,
                            CmpOp::Gte => v >= *value,
                            CmpOp::Lte => v <= *value,
                        }
                    } else { false }
                }).collect();
                Ok(json!(filtered))
            },
            LogicOp::FormatString { template, variables } => {
                let mut result = template.clone();
                for var in variables {
                    if let Ok(val) = state.get(&var.path) {
                        let s = match val {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            _ => val.to_string(),
                        };
                        result = result.replace(&format!("{{{}}}", var.key), &s);
                    }
                }
                Ok(json!(result))
            }
        }
    }
}

fn get_f64(state: &RuntimeState, path: &str) -> Result<f64, MetaError> {
    state.get(path)?
        .as_f64()
        .ok_or_else(|| MetaError::RuntimeError(format!("Value at {path} is not a number")))
}

fn get_array(state: &RuntimeState, path: &str) -> Result<Vec<Value>, MetaError> {
    state.get(path)?
        .as_array()
        .cloned()
        .ok_or_else(|| MetaError::RuntimeError(format!("Value at {path} is not an array")))
}