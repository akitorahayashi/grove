use clap::Subcommand;

use crate::AppError;
use crate::app::api;

#[derive(Subcommand)]
pub(super) enum LabelCommand {
    #[command(about = "Add a new label", visible_alias = "a")]
    Add { name: String },
    #[command(about = "List all labels", visible_alias = "ls")]
    List,
    #[command(about = "Delete a label", visible_alias = "rm")]
    Delete { name: String },
}

pub(super) fn run(command: LabelCommand) -> Result<(), AppError> {
    match command {
        LabelCommand::Add { name } => {
            api::label_add(&name)?;
            println!("Added label '{name}'");
        }
        LabelCommand::List => {
            let labels = api::label_list()?;

            println!("Stored labels:");
            if labels.is_empty() {
                println!("(none)");
            } else {
                for name in &labels {
                    println!("- {name}");
                }
            }
        }
        LabelCommand::Delete { name } => {
            api::label_delete(&name)?;
            println!("Deleted label '{name}'");
        }
    }

    Ok(())
}
