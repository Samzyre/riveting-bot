use std::io::Write;
use std::time::Duration;
use std::{env, fs};

use chrono::{DateTime, TimeZone, Utc};
use delay_timer::prelude::*;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::time;
use twilight_mention::Mention;
use twilight_model::id::marker::RoleMarker;
use twilight_model::id::Id;
use twilight_util::builder::embed;

use crate::commands::{CommandContext, CommandError, CommandResult};
use crate::utils::prelude::*;
use crate::{parser, Context};

/// Command: Manage community event schedule.
pub async fn scheduler(cc: CommandContext<'_>) -> CommandResult {
    // Send help.
    cc.http
        .create_message(cc.msg.channel_id)
        .content(
            "```add - add an event (format: !scheduler add <name> <year> <month> <day> <hour> \
             <minute> <second>), **time in UTC**\nrm - remove an event (format: !scheduler rm \
             <event_id>)```",
        )?
        .send()
        .await?;

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Event {
    id: u32,
    name: String,
    added_by: String,
    added_at: DateTime<Utc>,
    finishing_at: DateTime<Utc>,
}

/// Command: Add a community event to be scheduled.
/// Usage: scheduler add <name of event> <year> <month> <day> <hours> <minutes> <seconds>, time in UTC
pub async fn add(cc: CommandContext<'_>) -> CommandResult {
    let args = parser::parse_args(cc.args.trim())?;

    // Create a date from arguments
    let mut date_vec: Vec<u32> = Vec::new();

    for i in 1..7 {
        match args[i].parse::<u32>() {
            Ok(n) => date_vec.push(n),
            Err(e) => {
                cc.http
                    .create_message(cc.msg.channel_id)
                    .reply(cc.msg.id)
                    .content(&e.to_string())?
                    .send()
                    .await?;
                break;
            },
        }
    }

    let event_name = args
        .get(0)
        .ok_or(CommandError::MissingArgs)?
        .trim()
        .to_string();

    let time = Utc::now();
    let completion = Utc
        .ymd(date_vec[0] as i32, date_vec[1], date_vec[2])
        .and_hms(date_vec[3], date_vec[4], date_vec[5]);

    let completed_in = format!("<t:{}:R>", completion.timestamp());

    let query_user: String = cc.msg.id.get().to_string();

    // Generate a random file name.
    let rand_file_name: u32 = rand::thread_rng().gen();

    let event = Event {
        id: rand_file_name,
        name: event_name.clone(),
        added_by: query_user,
        added_at: time,
        finishing_at: completion,
    };

    // Push event into json file.
    let serialised_event: String = serde_json::to_string(&event).unwrap();

    fs::create_dir_all("./data/events")
        .map_err(|e| anyhow::anyhow!("Failed to create events dir: {}", e))?;

    fs::File::create(format!("./data/events/{}.json", rand_file_name))
        .map_err(|e| anyhow::anyhow!("Failed to create a file: {}", e))?;

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(format!("./data/events/{}.json", rand_file_name))
        .unwrap();

    write!(file, "{}", serialised_event)
        .map_err(|e| anyhow::anyhow!("Failed to write to file: {}", e))?;

    // Create and send an embed
    let embed = embed::EmbedBuilder::new()
        .title("Event added")
        .field(embed::EmbedFieldBuilder::new("Event name: ", event_name))
        .field(embed::EmbedFieldBuilder::new(
            "Starts at: ",
            format!("{completion}\n{completed_in}"),
        ))
        .field(embed::EmbedFieldBuilder::new(
            "Event ID: ",
            format!("{}", &rand_file_name),
        ))
        .color(0xed00fa)
        .build();

    cc.http
        .create_message(cc.msg.channel_id)
        .reply(cc.msg.id)
        .embeds(&[embed])?
        .send()
        .await?;

    Ok(())
}

/// Command: Remove a community event from the schedule.
/// Usage: scheduler rm <event_id>
pub async fn rm(cc: CommandContext<'_>) -> CommandResult {
    let args: Vec<&str> = cc.args.split(' ').collect();
    // If no arguments provided throw an error
    if args[0].is_empty() {
        cc.http
            .create_message(cc.msg.channel_id)
            .reply(cc.msg.id)
            .content("Specify event ID")?
            .send()
            .await?;
    } else {
        // Remove the event file, create and send an embed
        let embed = embed::EmbedBuilder::new()
            .title("Event removed")
            .field(embed::EmbedFieldBuilder::new(
                "Event ID: ",
                args[0].to_string(),
            ))
            .color(0xed00fa)
            .build();
        fs::remove_file(format!("./data/events/{}.json", &args[0]))
            .map_err(|e| anyhow::anyhow!("Failed to remove the file: {}", e))?;
        cc.http
            .create_message(cc.msg.channel_id)
            .reply(cc.msg.id)
            .embeds(&[embed])?
            .send()
            .await?;
    }

    Ok(())
}

/// Every `period` seconds check if there are any events that are scheduled for this period,
/// if there are any - start timer and send a message when it ends.
pub async fn handle_timer(ctx: Context, period: u64) -> AnyResult<()> {
    // Setting an interval
    let mut interval = time::interval(time::Duration::from_secs(period));

    loop {
        interval.tick().await;

        // Use period as the look-ahead time also, for now.
        check_schedule(&ctx, period).await?;
    }
}

/// Check if any events are happening in the next `look_ahead` seconds.
async fn check_schedule(ctx: &Context, look_ahead: u64) -> AnyResult<()> {
    // TODO Use a single file for events.

    std::fs::create_dir_all("./data/events")
        .map_err(|e| anyhow::anyhow!("Failed to create events folder: {}", e))?;

    let paths = fs::read_dir("./data/events").unwrap();

    let now: i64 = Utc::now().timestamp();
    let mut tasks = Vec::<Event>::new();

    // Loop through files and check if any of those upcoming events are within an hour from now.
    for path in paths {
        let current_file = path.unwrap().path().display().to_string();
        let string: String =
            String::from_utf8_lossy(&fs::read(&current_file).expect("Can't load the file"))
                .parse()
                .expect("Can't parse the file");

        let event: Event = serde_json::from_str(&string).unwrap();
        let finish_time = event.finishing_at.timestamp();

        // If such tasks are found, push them to a vector

        if finish_time - now < look_ahead as i64 {
            tasks.push(event);
        }
    }

    let mut futs = tokio::task::JoinSet::new();

    // Loop through the vector, set the timer and send a mention.
    for task in tasks {
        let finish_time = task.finishing_at.timestamp();
        let time_left = finish_time - now;

        fs::remove_file(format!("./data/events/{}.json", &task.id))?;

        futs.spawn(wait_for_starting(
            ctx.clone(),
            task,
            u64::try_from(time_left).unwrap_or(0),
        ));
    }

    // This will wait for all tasks to complete in this period.
    loop {
        match futs.join_one().await {
            // All done.
            Ok(None) => break,
            // Log if an error occurred inside event waiting.
            Err(e) => error!("Error in a scheduled event: {}", e),
            // One completed without errors.
            _ => (),
        }
    }

    Ok(())
}

async fn wait_for_starting(ctx: Context, event: Event, until: u64) -> AnyResult<()> {
    debug!("Starting a community event in {} seconds", until);

    time::sleep(Duration::from_secs(until)).await;

    let embed = embed::EmbedBuilder::new()
        .title(format!("{} is starting", &event.name))
        .description(format!("Starts at: {}", &event.finishing_at))
        .color(0xed00fa)
        .build();

    let announcement_role = env::var("ANNOUNCEMENT_ROLE")?;
    let announcement_channel = env::var("ANNOUNCEMENT_CHANNEL")?;

    let role_id = Id::<RoleMarker>::new(announcement_role.parse()?);

    let message = role_id.mention().to_string();

    ctx.http
        .create_message(Id::new(announcement_channel.parse()?))
        .content(&message)?
        .embeds(&[embed])?
        .send()
        .await?;

    Ok(())
}
