use clap::Subcommand;

use crate::AppError;
use crate::app::api;

#[derive(Subcommand)]
pub(super) enum LabelingCommand {
    #[command(about = "Attach a label to an item", visible_alias = "a")]
    Attach { item_id: String, label_name: String },
    #[command(about = "Detach a label from an item", visible_alias = "d")]
    Detach { item_id: String, label_name: String },
    #[command(about = "List labels for an item", visible_alias = "ls")]
    List { item_id: String },
    #[command(about = "Find items by label", visible_alias = "f")]
    Find {
        #[arg(long)]
        label: String,
    },
}

pub(super) fn run(command: LabelingCommand) -> Result<(), AppError> {
    match command {
        LabelingCommand::Attach { item_id, label_name } => {
            api::labeling_attach(&item_id, &label_name)?;
            println!("Attached label '{label_name}' to item '{item_id}'");
        }
        LabelingCommand::Detach { item_id, label_name } => {
            api::labeling_detach(&item_id, &label_name)?;
            println!("Detached label '{label_name}' from item '{item_id}'");
        }
        LabelingCommand::List { item_id } => {
            let labels = api::labeling_list(&item_id)?;

            println!("Labels for item '{item_id}':");
            if labels.is_empty() {
                println!("(none)");
            } else {
                for name in &labels {
                    println!("- {name}");
                }
            }
        }
        LabelingCommand::Find { label } => {
            let items = api::labeling_find(&label)?;

            println!("Items with label '{label}':");
            if items.is_empty() {
                println!("(none)");
            } else {
                for id in &items {
                    println!("- {id}");
                }
            }
        }
    }

    Ok(())
}
