mod error;
mod core {
    pub mod dsl;
    pub mod runtime;
}
mod ai {
    pub mod client;
    pub mod prompts;
    pub mod agents;
    pub mod schema_utils; // Registered here
}
mod orchestrator;

use dotenv::dotenv;
use orchestrator::Orchestrator;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::builder().filter_level(log::LevelFilter::Info).init();

    let orchestrator = Orchestrator::new();

    let prompt = r#"
        I need a financial tool for analysing project profitability.
        Input: 
        - A list of 'projects'. Each project has 'name', 'revenue', 'costs', and 'hours_worked'.
        - An 'overhead_rate' (hourly cost of overhead).
        
        Output:
        1. 'total_profit': Total Revenue - Total Costs - (Total Hours * Overhead Rate).
        2. 'most_profitable_project': Name of the project with highest raw profit (Revenue - Costs).
        3. 'profit_margin': Total Profit / Total Revenue (as a percentage).
        4. 'summary': A text string summarizing the results.
    "#;

    println!("🤖 META-AI SYSTEM INITIALIZED");
    println!("📝 Processing Request: \"{}\"\n", prompt.trim());

    let app = orchestrator.build_application(prompt).await?;

    println!("\n📦 PRODUCTION APP READY: {}", app.definition.name);
    println!("--------------------------------------------------");

    let real_data = json!({
        "overhead_rate": 50.0,
        "projects": [
            { "name": "Website Redesign", "revenue": 15000, "costs": 2000, "hours_worked": 100 },
            { "name": "Mobile App", "revenue": 40000, "costs": 15000, "hours_worked": 400 },
            { "name": "Consulting", "revenue": 5000, "costs": 0, "hours_worked": 20 }
        ]
    });

    match core::runtime::Runtime::execute(&app, real_data) {
        Ok(output) => {
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Err(e) => {
            eprintln!("❌ Runtime Error in Production: {e}");
        }
    }

    Ok(())
}