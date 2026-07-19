use clap::Subcommand;

use crate::AppError;
use crate::app::api;

#[derive(Subcommand)]
pub(super) enum ItemCommand {
    #[command(about = "Add a new item", visible_alias = "a")]
    Add {
        id: String,
        #[arg(short, long)]
        content: String,
    },
    #[command(about = "List all items", visible_alias = "ls")]
    List,
    #[command(about = "Delete an item", visible_alias = "rm")]
    Delete { id: String },
}

pub(super) fn run(command: ItemCommand) -> Result<(), AppError> {
    match command {
        ItemCommand::Add { id, content } => {
            api::item_add(&id, &content)?;
            println!("Added item '{id}'");
        }
        ItemCommand::List => {
            let items = api::item_list()?;

            println!("Stored items:");
            if items.is_empty() {
                println!("(none)");
            } else {
                for id in &items {
                    println!("- {id}");
                }
            }
        }
        ItemCommand::Delete { id } => {
            api::item_delete(&id)?;
            println!("Deleted item '{id}'");
        }
    }

    Ok(())
}
