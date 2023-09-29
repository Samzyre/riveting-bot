use twilight_gateway::Event;
use twilight_http::request::channel::reaction::RequestReactionType;
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{
    ActionRow, Button, ButtonStyle, SelectMenu, SelectMenuOption,
};
use twilight_model::channel::message::{Component, MessageFlags, ReactionType};
use twilight_model::channel::Message;
use twilight_model::gateway::payload::incoming::RoleUpdate;
use twilight_model::guild::Permissions;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};
use twilight_model::id::Id;
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::commands::prelude::*;
use crate::config::ReactionRole;
use crate::utils;
use crate::utils::prelude::*;

/// Command: Manage reaction-roles.
pub struct Roles;

impl Roles {
    pub fn command() -> impl Into<BaseCommand> {
        use crate::commands::builder::*;

        command("roles", "Manage reaction-roles.")
            .attach(Self::classic)
            .attach(Self::slash)
            .permissions(Permissions::ADMINISTRATOR)
            .option(
                sub("setup", "Setup a new reaction-roles message.")
                    .attach(Setup::classic)
                    .attach(Setup::slash),
            )
            .option(
                sub("edit", "Edit an existing reaction-roles message.")
                    .attach(Edit::classic)
                    .option(message("message", "Reaction-roles message to edit.").required()),
            )
    }

    async fn classic(_ctx: Context, _req: ClassicRequest) -> CommandResponse {
        todo!();
    }

    async fn slash(_ctx: Context, _req: SlashRequest) -> CommandResponse {
        todo!();
    }
}

/// Command: Setup a reaction-roles message.
struct Setup;

impl Setup {
    async fn uber(
        ctx: Context,
        guild_id: Id<GuildMarker>,
        channel_id: Id<ChannelMarker>,
        author_id: Id<UserMarker>,
    ) -> CommandResult<()> {
        let Some(mappings) =
            roles_setup_process(&ctx, guild_id, channel_id, author_id, None).await?
        else {
            return Ok(()); // Canceled or whatever.
        };

        let output_content = output_message_content(&ctx, guild_id, &mappings).await?;
        let output = ctx
            .http
            .create_message(channel_id)
            .content(&output_content)?
            .send()
            .await?;

        add_reactions_to_message(&ctx, &mappings, &output).await?;

        register_reaction_roles(&ctx, guild_id, output.channel_id, output.id, mappings)?;

        Ok(())
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        let Some(guild_id) = req.message.guild_id else {
            return Err(CommandError::Disabled);
        };

        req.clear(&ctx).await?;

        Self::uber(ctx, guild_id, req.message.channel_id, req.message.author.id)
            .await
            .map(|_| Response::none())
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        let Some(guild_id) = req.interaction.guild_id else {
            return Err(CommandError::Disabled);
        };

        let Some(channel) = req.interaction.channel.as_ref() else {
            return Err(CommandError::Disabled);
        };

        let Some(author_id) = req.interaction.author_id() else {
            return Err(CommandError::MissingArgs);
        };

        req.clear(&ctx).await?;

        Self::uber(ctx, guild_id, channel.id, author_id)
            .await
            .map(|_| Response::none())
    }
}

/// Command: Edit a reaction-roles mapping.
struct Edit;

impl Edit {
    async fn uber(ctx: Context, req: ClassicRequest) -> CommandResult<()> {
        let Some(guild_id) = req.message.guild_id else {
            return Err(CommandError::Disabled);
        };

        let Some(replied) = &req.message.referenced_message else {
            return Err(CommandError::MissingReply);
        };

        // Ignore if replied message is not from this bot.
        if replied.author.id != ctx.user.id {
            return Err(CommandError::UnexpectedArgs(
                "Replied message is not from this bot".to_string(),
            ));
        }

        let reaction_roles = ctx
            .config
            .guild(guild_id)
            .reaction_roles(replied.channel_id, replied.id)
            .with_context(|| {
                CommandError::UnexpectedArgs("Message is not a reaction-roles post".to_string())
            })?;

        let author_id = req.message.author.id;
        let channel_id = req.message.channel_id;

        let Some(mappings) =
            roles_setup_process(&ctx, guild_id, channel_id, author_id, Some(reaction_roles))
                .await?
        else {
            return Ok(()); // Canceled or whatever.
        };

        let output_content = output_message_content(&ctx, guild_id, &mappings).await?;

        // NOTE: This will just overwrite all content of the original message.
        let output = ctx
            .http
            .update_message(replied.channel_id, replied.id)
            .content(Some(&output_content))?
            .send()
            .await?;

        add_reactions_to_message(&ctx, &mappings, &output).await?;

        register_reaction_roles(&ctx, guild_id, output.channel_id, output.id, mappings)?;

        Ok(())
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        req.clear(&ctx).await?;
        Self::uber(ctx, req).await.map(|_| Response::none())
    }
}

/// Content to show on the final message.
async fn output_message_content(
    ctx: &Context,
    guild_id: Id<GuildMarker>,
    mappings: &[ReactionRole],
) -> AnyResult<String> {
    let list = display_emoji_roles(ctx, guild_id, mappings).await?;
    Ok(indoc::formatdoc! {"
        React to give yourself some roles:

        {}
        ",
        list
    })
}

/// Write to config.
fn register_reaction_roles(
    ctx: &Context,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    mappings: Vec<ReactionRole>,
) -> AnyResult<()> {
    ctx.config
        .guild(guild_id)
        .add_reaction_roles(channel_id, message_id, mappings)
}

/// Cognitive overload.
async fn roles_setup_process(
    ctx: &Context,
    guild_id: Id<GuildMarker>,
    channel_id: Id<ChannelMarker>,
    author_id: Id<UserMarker>,
    preset: Option<Vec<ReactionRole>>,
) -> Result<Option<Vec<ReactionRole>>, CommandError> {
    let info_text = indoc::formatdoc! {"
        **Reaction-roles setup**
        
            *Step 1:*  React to this message.
            *Step 2:*  Select a role from the dropdown menu.
            *Step 3:*  Repeat until you are happy with the role mappings.
            *Step 4:*  Press **Done** to finalize the reaction-roles message.
        
        If any role is not displayed in the list, it may be too stronk for the bot.
    "};

    let interaction = ctx.interaction();
    let mut mappings = preset.unwrap_or_default();

    // Initial content of the controller message.
    let content = {
        let list = display_emoji_roles(ctx, guild_id, &mappings).await?;
        format!("{info_text}\n{list}")
    };

    // Setup message with controls.
    let mut controller = ctx
        .http
        .create_message(channel_id)
        .content(&content)?
        .components(&controller_components(true))?
        .send()
        .await?;

    // Add any previous reactions if this is an edit.
    add_reactions_to_message(ctx, &mappings, &controller).await?;

    let controller_mci = loop {
        // Future that waits for controller button press.
        let controller_fut = ctx
            .standby
            .wait_for_component(controller.id, move |event: &Interaction| {
                event.author_id() == Some(author_id)
            });

        // Future that waits for a reaction add or remove.
        let reaction_fut = {
            let id = controller.id;

            ctx.standby
                .wait_for(guild_id, move |event: &Event| match event {
                    Event::ReactionAdd(r) => {
                        r.message_id == id && r.channel_id == channel_id && r.user_id == author_id
                    },
                    Event::ReactionRemove(r) => {
                        r.message_id == id && r.channel_id == channel_id && r.user_id == author_id
                    },
                    _ => false,
                })
        };

        // Wait for a reaction or a controller button.
        let event = tokio::select! {
            biased;
            event = reaction_fut => event?, // Proceed with the reaction event.
            mci = controller_fut => break mci?, // Exit loop with button interaction.
        };

        match event {
            Event::ReactionAdd(added) => {
                // Show roles dropdown list and add a role mapping.

                // If already mapped, ignore it.
                if mappings
                    .iter()
                    .any(|ReactionRole { emoji, .. }| utils::reaction_type_eq(emoji, &added.emoji))
                {
                    continue;
                }

                let components = dropdown_components(ctx, guild_id, &added.emoji).await?;

                // Gray out controller buttons.
                update_controller(ctx, &mut controller, None, false).await?;

                // Create dropdown list interaction.
                let dropdown = ctx
                    .http
                    .create_message(channel_id)
                    .components(&components)?
                    .send()
                    .await?;

                // Wait for user to select an option.
                let list_mci = ctx
                    .standby
                    .wait_for_component(dropdown.id, move |event: &Interaction| {
                        event.author_id() == Some(author_id)
                    })
                    .await?;

                let resp = InteractionResponse {
                    kind: InteractionResponseType::DeferredUpdateMessage,
                    data: Some(InteractionResponseData::default()),
                };

                // Acknowledge the interaction.
                interaction
                    .create_response(list_mci.id, &list_mci.token, &resp)
                    .await?;

                // Delete the dropdown message.
                interaction.delete_response(&list_mci.token).await?;

                let choice = match list_mci.data {
                    Some(InteractionData::MessageComponent(mut data)) => {
                        data.values.pop().unwrap_or_default()
                    },
                    _ => {
                        error!("Received invalid interaction data in reaction-roles setup");
                        "cancel".to_string() // Because of this error, act as if it was canceled.
                    },
                };

                if choice == "cancel" {
                    // Canceling, re-enable controller buttons.
                    update_controller(ctx, &mut controller, None, true).await?;

                    // Remove canceled reaction.
                    let request_emoji = request_from_emoji(&added.emoji);

                    ctx.http
                        .delete_all_reaction(controller.channel_id, controller.id, &request_emoji)
                        .await?;

                    continue;
                }

                match choice.parse::<Id<RoleMarker>>() {
                    Ok(role_id) => {
                        // Save the choice.
                        mappings.push(ReactionRole::new(added.emoji.to_owned(), role_id));

                        // Create a message that lists all roles that have been added so far.
                        let list = display_emoji_roles(ctx, guild_id, &mappings).await?;
                        let content = format!("{info_text}\n{list}");

                        // Update the controller message and re-enable controller buttons.
                        update_controller(ctx, &mut controller, Some(&content), true).await?;
                    },
                    Err(e) => {
                        // Error parsing the choice.
                        warn!("Could not parse role choice: {e}");

                        // Update the controller message and re-enable controller buttons.
                        update_controller(ctx, &mut controller, None, true).await?;
                    },
                }
            },
            Event::ReactionRemove(removed) => {
                // Remove any mapping that has this emoji.
                let _ = mappings
                    .iter()
                    .enumerate()
                    .find(|(_, r)| utils::reaction_type_eq(&r.emoji, &removed.emoji))
                    .map(|(idx, _)| idx)
                    .map(|idx| mappings.remove(idx));

                let list = display_emoji_roles(ctx, guild_id, &mappings).await?;
                let content = format!("{info_text}\n{list}");

                update_controller(ctx, &mut controller, Some(&content), true).await?;

                let request_emoji = request_from_emoji(&removed.emoji);

                ctx.http
                    .delete_all_reaction(controller.channel_id, controller.id, &request_emoji)
                    .await?;
            },
            _ => {
                warn!("Unexpected event type in reaction-roles setup mapping");
                continue;
            },
        }
    };

    let data = match controller_mci.data {
        Some(InteractionData::MessageComponent(data)) => data,
        _ => Err(anyhow::anyhow!(
            "Received invalid interaction data in reaction-roles setup"
        ))?,
    };

    // If cancelled, no reaction-roles were added or all were removed.
    if data.custom_id == "roles_cancel" || mappings.is_empty() {
        // Delete the controller message.
        ctx.http
            .delete_message(controller.channel_id, controller.id)
            .await?;

        // Nothing more to be done here.
        return Ok(None);
    }

    let resp = InteractionResponse {
        kind: InteractionResponseType::ChannelMessageWithSource,
        data: Some(
            InteractionResponseDataBuilder::new()
                .flags(MessageFlags::EPHEMERAL)
                .content(format!(
                    "Done; You can use `{prefix}bot edit` command to edit the message content, or \
                     `{prefix}roles edit` command to edit the role mappings.",
                    prefix = ctx.config.classic_prefix(Some(guild_id))?
                ))
                .build(),
        ),
    };

    interaction
        .create_response(controller_mci.id, &controller_mci.token, &resp)
        .await?;

    // Delete the controller message.
    ctx.http
        .delete_message(controller.channel_id, controller.id)
        .await?;

    Ok(Some(mappings))
}

/// Does what it says.
async fn add_reactions_to_message(
    ctx: &Context,
    mappings: &[ReactionRole],
    message: &Message,
) -> AnyResult<()> {
    for rr in mappings.iter() {
        let request_emoji = request_from_emoji(&rr.emoji);

        ctx.http
            .create_reaction(message.channel_id, message.id, &request_emoji)
            .await?;
    }

    Ok(())
}

/// Creates components for reaction-roles setup dropdown selection.
async fn dropdown_components(
    ctx: &Context,
    guild_id: Id<GuildMarker>,
    emoji: &ReactionType,
) -> AnyResult<Vec<Component>> {
    // Get all available roles. Try cache, otherwise fetch.
    let roles = match ctx.cache.guild_roles(guild_id) {
        Some(role_ids) => {
            ctx.roles_from(guild_id, &role_ids.iter().copied().collect::<Vec<_>>())
                .await?
        },
        None => ctx.http.roles(guild_id).send().await?,
    };

    // Find the highest role that the bot has.
    let bot_role = {
        let bot_roles = match ctx.cache.member(guild_id, ctx.user.id) {
            Some(m) => m.value().roles().to_vec(),
            None => {
                ctx.http
                    .guild_member(guild_id, ctx.user.id)
                    .send()
                    .await?
                    .roles
            },
        };

        roles
            .iter()
            .filter(|r| bot_roles.contains(&r.id))
            .max()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Could not find maximum role for bot"))?
    };

    // Role options to display.
    let role_opts = roles
        .into_iter()
        // Filter out `@everyone` role.
        .filter(|r| r.id != guild_id.cast())
        // Filter out roles that are higher or same as the bot's role.
        .filter(|r| *r < bot_role)
        // Filter out roles that are integration managed.
        .filter(|r| !r.managed)
        // Filter out roles with admin permissions, as a precaution.
        .filter(|r| !r.permissions.contains(Permissions::ADMINISTRATOR))
        .map(|r| SelectMenuOption {
            default: false,
            description: None,
            emoji: Some(emoji.to_owned()),
            label: r.name,
            value: r.id.to_string(),
        }).chain([
            SelectMenuOption {
                default: false,
                description: Some("Cancel".to_string()),
                emoji: None,
                label: "-".to_string(), // Empty, but not.
                value: "cancel".to_string(),
            }
        ])
        .collect::<Vec<_>>();

    // Roles dropdown list.
    Ok(vec![Component::ActionRow(ActionRow {
        components: vec![Component::SelectMenu(SelectMenu {
            custom_id: "role".to_string(),
            disabled: false,
            max_values: Some(1),
            min_values: Some(1),
            options: role_opts,
            placeholder: Some("Select a role to add".to_string()),
        })],
    })])
}

fn controller_components(enabled: bool) -> Vec<Component> {
    vec![Component::ActionRow(ActionRow {
        components: vec![
            // Button to finish adding reactions.
            Component::Button(Button {
                custom_id: Some("roles_done".to_string()),
                disabled: !enabled,
                emoji: None,
                label: Some("Done".to_string()),
                style: ButtonStyle::Success,
                url: None,
            }),
            // Button to cancel the process.
            Component::Button(Button {
                custom_id: Some("roles_cancel".to_string()),
                disabled: !enabled,
                emoji: None,
                label: Some("Cancel".to_string()),
                style: ButtonStyle::Danger,
                url: None,
            }),
        ],
    })]
}

/// Enable or disable setup message controls.
/// If `content` is `None`, previous content is used.
async fn update_controller(
    ctx: &Context,
    controller: &mut Message,
    content: Option<&str>,
    enabled: bool,
) -> AnyResult<()> {
    *controller = ctx
        .http
        .update_message(controller.channel_id, controller.id)
        .content(content.or(Some(&controller.content)))?
        .components(Some(&controller_components(enabled)))?
        .send()
        .await?;

    Ok(())
}

/// Creates a string with `<emoji> : <role>` pairs.
async fn display_emoji_roles(
    ctx: &Context,
    guild_id: Id<GuildMarker>,
    emoji_roles: &[ReactionRole],
) -> Result<String, CommandError> {
    let mut emoji_roles_msg = String::new();

    for ReactionRole { emoji, role } in emoji_roles {
        let (Ok(emoji) | Err(emoji)) = utils::display_reaction_emoji(emoji);

        emoji_roles_msg.push_str(&emoji);
        emoji_roles_msg.push_str(" : `");

        // Try to get a name from the cache.
        let cached_name = ctx.cache.role(*role).map(|r| r.name.to_string());
        let name = match cached_name {
            Some(n) => n,
            None => {
                let roles = ctx.http.roles(guild_id).send().await?;

                // Name of this role.
                let this = roles
                    .iter()
                    .find(|r| r.id == *role)
                    .map(|r| r.name.to_string())
                    .unwrap_or_else(|| role.to_string()); // Use id, if all else fails.

                // Manually update the cache.
                for role in roles {
                    ctx.cache.update(&RoleUpdate { guild_id, role });
                }

                this
            },
        };

        emoji_roles_msg.push_str(&name);
        emoji_roles_msg.push_str("`\n");
    }

    Ok(emoji_roles_msg)
}

fn request_from_emoji(r: &ReactionType) -> RequestReactionType {
    match r {
        ReactionType::Custom { id, name, .. } => RequestReactionType::Custom {
            id: *id,
            name: name.as_deref(),
        },
        ReactionType::Unicode { name } => RequestReactionType::Unicode { name },
    }
}
