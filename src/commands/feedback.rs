use serenity::builder::CreateApplicationCommand;
use serenity::model::prelude::command::CommandOptionType;
use serenity::model::prelude::interaction::application_command::{
    CommandDataOption,
};

pub fn run(_options: &[CommandDataOption]) -> String {
        "Please send 1.2gi first.".to_string()
}

pub fn register(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command.name("feedback").description("send a feedback to the alien.").create_option(|option| {
        option
            .name("text")
            .description("feedback input text.")
            .kind(CommandOptionType::String)
            .required(true)
    })
}
