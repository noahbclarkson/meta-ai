use crate::ai::agents::AgentSwarm;
use crate::core::dsl::AppProgram;
use crate::core::runtime::Runtime;
use crate::error::MetaError;
use serde_json::Value;

pub struct Orchestrator {
    swarm: AgentSwarm,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self { swarm: AgentSwarm::new() }
    }

    pub async fn build_application(&self, user_request: &str) -> Result<AppProgram, MetaError> {
        log::info!("🏗️  Phase 1: Architecture");
        let definition = self.swarm.define_app(user_request).await?;
        log::info!("   -> Defined: {}", definition.name);

        log::info!("🏗️  Phase 2: Development");
        let mut program = self.swarm.write_logic(&definition).await?;
        log::info!("   -> Generated {} steps of logic", program.steps.len());

        log::info!("🏗️  Phase 3: QA & Testing");
        let tests = self.swarm.generate_tests(&definition).await?;
        
        // Validation Loop
        let max_retries = 3;
        for attempt in 1..=max_retries {
            log::info!("   🛡️  Validation Run #{attempt}...");
            
            let mut all_passed = true;
            let mut error_report = String::new();

            for test in &tests {
                // ROBUSTNESS: Handle case where LLM returns input as a stringified JSON string
                let input_val = if let Some(input_str) = test.input.as_str() {
                    match serde_json::from_str::<Value>(input_str) {
                        Ok(v) => v,
                        Err(_) => test.input.clone(),
                    }
                } else {
                    test.input.clone()
                };

                match Runtime::execute(&program, input_val.clone()) {
                    Ok(output) => {
                        log::info!("      ✅ Test '{}' Passed", test.name);
                        log::info!("         Input:  {}", truncate_json(&input_val));
                        log::info!("         Output: {}", truncate_json(&output));
                    },
                    Err(e) => {
                        log::error!("      ❌ Test '{}' Failed: {}", test.name, e);
                        all_passed = false;
                        error_report = format!("Test '{}' failed: {}", test.name, e);
                        break; // Stop testing, go to fix
                    }
                }
            }

            if all_passed {
                log::info!("🎉 Program Verified Successfully!");
                return Ok(program);
            }

            if attempt < max_retries {
                log::warn!("   🔧 Invoking Fixer Agent...");
                program = self.swarm.fix_program(&program, &definition, &error_report).await?;
            }
        }

        Err(MetaError::ValidationFailed("Failed to generate valid program after max retries".into()))
    }
}

fn truncate_json(v: &Value) -> String {
    let s = serde_json::to_string(v).unwrap_or_default();
    if s.len() > 300 {
        format!("{}... (len: {})", &s[..300], s.len())
    } else {
        s
    }
}