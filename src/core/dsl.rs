use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ConstantValue {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MathOp { Add, Subtract, Multiply, Divide }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FormatVariable {
    #[schemars(description = "The placeholder name in the template (without braces).")]
    pub key: String,
    #[schemars(description = "The path to the data value.")]
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
#[schemars(description = "An atomic operation. Select exactly one 'op'.")]
pub enum LogicOp {
    #[schemars(description = "Read a value from the state.")]
    Get { path: String },
    
    #[schemars(description = "Set a constant value.")]
    Constant { value: ConstantValue },
    
    #[schemars(description = "Extract a field from a list of objects.")]
    Pluck { path: String, key: String },

    // Math
    Add { a: String, b: String },
    Subtract { a: String, b: String },
    Multiply { a: String, b: String },
    Divide { a: String, b: String },
    
    #[schemars(description = "Math on list items.")]
    Calculate {
        list_path: String,
        output_field: String,
        operator: MathOp,
        a_field: String,
        b_field: String,
    },

    // Aggregations
    Sum { list_path: String, field: Option<String> },
    Count { list_path: String },
    Min { list_path: String, field: Option<String> },
    Max { list_path: String, field: Option<String> },

    // Logic
    FilterNumeric {
        list_path: String,
        field: Option<String>,
        operator: CmpOp,
        value: f64
    },
    
    Sort {
        list_path: String,
        field: String,
        descending: bool,
    },
    
    #[schemars(description = "Create a formatted string.")]
    FormatString {
        #[schemars(description = "Template like 'Hello {name}'.")]
        template: String,
        #[schemars(description = "List of variables to replace placeholders.")]
        variables: Vec<FormatVariable> 
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp { Gt, Lt, Eq, Gte, Lte }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LogicStep {
    pub id: String,
    pub description: String,
    pub operation: LogicOp,
    pub output_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppDefinition {
    pub name: String,
    pub description: String,
    #[schemars(skip)]
    pub input_schema: serde_json::Value,
    #[schemars(skip)]
    pub output_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppProgram {
    pub definition: AppDefinition,
    pub steps: Vec<LogicStep>,
}