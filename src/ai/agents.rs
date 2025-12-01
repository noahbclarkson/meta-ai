use super::client::GeminiClient;
use super::prompts;
use super::schema_utils;
use crate::core::dsl::{AppDefinition, AppProgram, LogicStep};
use crate::error::MetaError;
use schemars::schema_for;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct AgentSwarm {
    client: GeminiClient,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct TestCase {
    pub name: String,
    pub input: Value,
    pub expected_output_keys: Vec<String>,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AppDefinitionResponse {
    pub name: String,
    pub description: String,
    pub input_schema_json: String,
    pub output_schema_json: String,
}

impl AgentSwarm {
    pub fn new() -> Self {
        Self { client: GeminiClient::new() }
    }

    pub async fn define_app(&self, user_request: &str) -> Result<AppDefinition, MetaError> {
        let raw_schema = schema_for!(AppDefinitionResponse);
        let raw_schema_text = serde_json::to_string_pretty(&raw_schema).unwrap();
        let clean_schema_val = schema_utils::clean_schema(raw_schema).map_err(MetaError::JsonError)?;

        let system_prompt = format!("{}\n\nREQUIRED OUTPUT SCHEMA:\n{}", prompts::ARCHITECT_PROMPT, raw_schema_text);

        let resp = self.client.generate(&system_prompt, user_request, Some(clean_schema_val), "Architecture").await?;
        
        let dto: AppDefinitionResponse = serde_json::from_str(&resp).map_err(|e| {
            MetaError::ValidationFailed(format!("Architect parse failed: {}", e))
        })?;

        let input_schema = parse_json_string(&dto.input_schema_json, "input_schema")?;
        let output_schema = parse_json_string(&dto.output_schema_json, "output_schema")?;

        Ok(AppDefinition {
            name: dto.name,
            description: dto.description,
            input_schema,
            output_schema,
        })
    }

    pub async fn write_logic(&self, definition: &AppDefinition) -> Result<AppProgram, MetaError> {
        // We use the raw schema text for the PROMPT, but pass None for the API schema.
        let raw_schema = schema_for!(Vec<LogicStep>);
        let raw_schema_text = serde_json::to_string_pretty(&raw_schema).unwrap();

        let system = format!(
            r#"
            You are a Backend Logic Developer.
            
            GOAL: Write a JSON Logic Program that transforms Input to Output.
            
            STRICT SCHEMA DOCUMENTATION:
            {}
            
            IMPORTANT EXAMPLES:
            
            1. Math Operation:
            {{
              "id": "calc_tax",
              "description": "Calculate tax",
              "operation": {{
                "op": "multiply",
                "a": "/revenue", 
                "b": "/tax_rate"
              }},
              "output_path": "/tax_amount"
            }}

            2. Format String (CRITICAL):
            {{
              "id": "summary",
              "description": "Make summary",
              "operation": {{
                "op": "format_string",
                "template": "Project {{name}} made ${{profit}}.",
                "variables": [
                   {{ "key": "name", "path": "/project/name" }},
                   {{ "key": "profit", "path": "/project/profit" }}
                ]
              }},
              "output_path": "/summary"
            }}
            
            INSTRUCTIONS:
            1. Return ONLY the JSON array of steps.
            2. Use the 'op' field to define the operation type.
            3. MATH OPS: Operands 'a' and 'b' MUST BE PATH STRINGS (e.g., "/revenue"). To use a number, use 'constant' op first.
            4. FORMAT_STRING: 'variables' must be an ARRAY OF OBJECTS (key/path).
            "#,
            raw_schema_text
        );

        let initial_user_prompt = format!(
            "App Name: {}\nInput Schema: {}\nOutput Schema: {}\n\nGenerate the logic.",
            definition.name,
            serde_json::to_string_pretty(&definition.input_schema).unwrap(),
            serde_json::to_string_pretty(&definition.output_schema).unwrap()
        );

        let mut user = initial_user_prompt.clone();
        let max_retries = 3;

        for attempt in 1..=max_retries {
            // Passing None for schema to avoid strict mode parsing issues with recursion
            let json_text = self.client.generate(&system, &user, None, "Development").await?;

            match serde_json::from_str::<Vec<LogicStep>>(&json_text) {
                Ok(steps) => {
                    return Ok(AppProgram {
                        definition: definition.clone(),
                        steps,
                    });
                }
                Err(e) => {
                    log::warn!("Attempt {}/{} failed to parse logic: {}", attempt, max_retries, e);
                    if attempt == max_retries {
                        return Err(MetaError::ValidationFailed(format!("Logic parse failed: {}", e)));
                    }
                    user = format!(
                        "{}\n\n⚠️ PREVIOUS ATTEMPT FAILED: {}.\n\
                        Check your JSON syntax:\n\
                        1. 'FormatString' variables must be [ {{ \"key\": \"...\", \"path\": \"...\" }} ]. NOT strings.\n\
                        2. Math operands ('a', 'b') must be PATH STRINGS. To use a number, use 'op': 'constant' first.\n\
                        Try again.", 
                        initial_user_prompt, 
                        e
                    );
                }
            }
        }
        
        Err(MetaError::ValidationFailed("Max logic retries exceeded".into()))
    }

    pub async fn generate_tests(&self, definition: &AppDefinition) -> Result<Vec<TestCase>, MetaError> {
        let raw_schema = schema_for!(Vec<TestCase>);
        let raw_schema_text = serde_json::to_string_pretty(&raw_schema).unwrap();
        let clean_schema_val = schema_utils::clean_schema(raw_schema).map_err(MetaError::JsonError)?;

        let system = format!("{}\n\nREQUIRED SCHEMA:\n{}", prompts::QA_PROMPT, raw_schema_text);

        let user = format!(
            "Input Schema: {}\nGenerate 3 diverse test cases.",
            serde_json::to_string_pretty(&definition.input_schema).unwrap()
        );
        
        let resp = self.client.generate(&system, &user, Some(clean_schema_val), "QA").await?;
        serde_json::from_str(&resp).map_err(|e| {
            MetaError::ValidationFailed(format!("Tests parse failed: {}", e))
        })
    }

    pub async fn fix_program(&self, program: &AppProgram, definition: &AppDefinition, error_log: &str) -> Result<AppProgram, MetaError> {
        let raw_schema = schema_for!(Vec<LogicStep>);
        let raw_schema_text = serde_json::to_string_pretty(&raw_schema).unwrap();

        let system = format!(
            "{}\n\nSTRICT SCHEMA DOCUMENTATION:\n{}",
            prompts::FIXER_PROMPT,
            raw_schema_text
        );

        let user = format!(
            "CONTEXT:\nApp Name: {}\nInput Schema: {}\n\nCurrent Steps: {}\n\nRuntime Error: {}\n\n\
            INSTRUCTIONS:\n\
            1. Return the FIXED steps array.\n\
            2. 'FormatString': use Array [ {{ \"key\": \"...\", \"path\": \"...\" }} ].\n\
            3. Math Operands: MUST be strings (paths).",
            definition.name,
            serde_json::to_string_pretty(&definition.input_schema).unwrap(),
            serde_json::to_string_pretty(&program.steps).unwrap(),
            error_log
        );

        // Passing None for schema
        let new_steps: Vec<LogicStep> = serde_json::from_str(
            &self.client.generate(&system, &user, None, "Fixer").await?
        ).map_err(|e| {
            MetaError::ValidationFailed(format!("Fixer parse failed: {}", e))
        })?;
        
        let mut new_program = program.clone();
        new_program.steps = new_steps;
        Ok(new_program)
    }
}

fn parse_json_string(s: &str, field_name: &str) -> Result<Value, MetaError> {
    let trimmed = s.trim();
    let content = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let sanitized: String = content.chars().map(|c| {
        if c.is_control() { ' ' } else { c }
    }).collect();

    let json_str = if let Some(start) = sanitized.find('{') {
        if let Some(end) = sanitized.rfind('}') {
            if end > start {
                &sanitized[start..=end]
            } else {
                &sanitized
            }
        } else {
            &sanitized
        }
    } else {
        &sanitized
    };

    serde_json::from_str(json_str).map_err(|e| {
        MetaError::ValidationFailed(format!(
            "Failed to parse {} string. Error: {}. Content was: {}", 
            field_name, e, json_str
        ))
    })
}