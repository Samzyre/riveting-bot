use chrono::{DateTime, FixedOffset, Utc};
use riveting_bot::commands::prelude::*;
use riveting_bot::utils::prelude::*;
use twilight_mention::timestamp::{Timestamp, TimestampStyle};
use twilight_mention::Mention;
use twilight_model::channel::message::Embed;
use twilight_util::builder::embed::{self, EmbedFieldBuilder, EmbedFooterBuilder};

// dateparser examples: https://github.com/waltzofpearls/dateparser#accepted-date-formats

/// If your timezone is something else, unlucky.
const TIMEZONES: [(&str, &str); 24] = [
    ("UTC-11", "-11"),
    ("UTC-10", "-10"),
    ("UTC-9", "-9"),
    ("UTC-8", "-8"),
    ("UTC-7", "-7"),
    ("UTC-6", "-6"),
    ("UTC-5", "-5"),
    ("UTC-4", "-4"),
    ("UTC-3", "-3"),
    ("UTC-2", "-2"),
    ("UTC-1", "-1"),
    ("UTC+0", "+0"),
    ("UTC+1", "+1"),
    ("UTC+2", "+2"),
    ("UTC+3", "+3"),
    ("UTC+4", "+4"),
    ("UTC+5", "+5"),
    ("UTC+6", "+6"),
    ("UTC+7", "+7"),
    ("UTC+8", "+8"),
    ("UTC+9", "+9"),
    ("UTC+10", "+10"),
    ("UTC+11", "+11"),
    ("UTC+12", "+12"),
];

/// Command: Display a discord timestamp.
pub struct Time;

impl Time {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("time", "Display a discord timestamp.")
            .attach(Self::classic)
            .attach(Self::slash)
            .option(string("expression", "Time expression to evaluate."))
            .option(
                string(
                    "timezone",
                    "Your timezone offset (ignored if tz in expression).",
                )
                .choices(TIMEZONES),
            )
            .dm()
            .help(indoc::formatdoc! {"
                Format examples: https://github.com/waltzofpearls/dateparser#accepted-date-formats
                    yyyy-mm-dd hh:mm:ss z   Mon dd, yyyy, hh:mm:ss      mm/dd/yyyy hh:mm:ss
                    yyyy-mm-dd hh:mm:ss     Mon dd, yyyy hh:mm:ss z     mm/dd/yyyy
                    yyyy-mm-dd z            Mon dd, yyyy                yyyy/mm/dd hh:mm:ss
                    yyyy-mm-dd              Mon dd hh:mm:ss             yyyy/mm/dd
                    hh:mm:ss z              dd Mon yyyy hh:mm:ss        mm.dd.yyyy
                    hh:mm:ss                dd Mon yyyy                 yyyy.mm.dd
                    yyyy-mon-dd             rfc3339, rfc2822, unix timestamp                    
                "
            })
    }

    async fn uber(args: Args) -> CommandResult<Embed> {
        let expr = args.string("expression").unwrap_or_default();

        let now = args
            .string("timezone")
            .and_then(|val| Ok(timezone(&val)?))
            .unwrap_or_else(|_| Utc::now().into());

        let parsed = if expr.trim().is_empty() {
            now
        } else {
            dateparser::parse_with(expr.trim(), &now.timezone(), now.time())
                .with_context(|| format!("Time expression '{}' is not valid", expr.trim()))?
                .fixed_offset()
        };

        let unix = parsed.timestamp() as _;
        let long = Timestamp::new(unix, Some(TimestampStyle::LongDateTime));
        let relative = Timestamp::new(unix, Some(TimestampStyle::RelativeTime));
        let footer = format!("Copypasta: {} {}", long.mention(), relative.mention());

        Ok(embed::EmbedBuilder::new()
            .color(0xFFAA44)
            .field(EmbedFieldBuilder::new("Date & Time", long.mention().to_string()).inline())
            .field(EmbedFieldBuilder::new("Relative", relative.mention().to_string()).inline())
            .footer(EmbedFooterBuilder::new(footer))
            .build())
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        let embed = Self::uber(req.args).await?;

        ctx.http
            .create_message(req.message.channel_id)
            .reply(req.message.id)
            .embeds(&[embed])?
            .await?;

        Ok(Response::none())
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        let embed = Self::uber(req.args).await?;

        ctx.interaction()
            .update_response(&req.interaction.token)
            .embeds(Some(&[embed]))?
            .await?;

        Ok(Response::none())
    }
}

fn timezone(zone: &str) -> AnyResult<DateTime<FixedOffset>> {
    let hour = 3600; // Seconds.
    let offset = hour * zone.trim().parse::<i32>()?;
    let offset = FixedOffset::east_opt(offset)
        .or_else(|| FixedOffset::west_opt(offset))
        .context("Invalid timezone offset")?;
    Ok(Utc::now().with_timezone(&offset))
}
