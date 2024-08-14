use chrono::NaiveTime;
use riveting_bot::commands::prelude::*;
use riveting_bot::utils::prelude::*;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, EmbedFooterBuilder};

/// Command: Calculate fuel required.
pub struct Fuel;

impl Fuel {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("fuel", "Calculate race fuel required.")
            .attach(Self::slash)
            .option(
                integer("stint-minutes", "Length of the race or stint in minutes.")
                    .required()
                    .min(1),
            )
            .option(
                integer("lap-minutes", "Lap time minutes.")
                    .required()
                    .min(0)
                    .max(30),
            )
            .option(
                number(
                    "lap-seconds",
                    "Lap time seconds (and optionally milliseconds as decimal).",
                )
                .required()
                .min(0.0)
                .max(59.9999),
            )
            .option(
                number("consumption", "Fuel consumption in litres per lap.")
                    .required()
                    .min(0.1)
                    .max(100.0),
            )
            .dm()
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        let stint = req.args.integer("stint-minutes")?;
        let minutes = req.args.integer("lap-minutes")? as u32;
        let seconds = req.args.number("lap-seconds")?;
        let consumption = req.args.number("consumption")?;

        let length_in_seconds = (stint * 60) as f64;
        let laptime_in_seconds = (minutes * 60) as f64 + seconds;

        let amount_of_laps = length_in_seconds / laptime_in_seconds;
        let fuel_needed = amount_of_laps.ceil() * consumption;

        // Laptime faster than this adds another lap.
        let min_lap_seconds = length_in_seconds / amount_of_laps.ceil();
        let min_lap_minutes = (min_lap_seconds / 60.0) as u32;
        let min_laptime = naive_time_millis(min_lap_minutes, min_lap_seconds)
            .unwrap_or_default()
            .format("%M:%S%.3f");

        let mut laps_go_minus = amount_of_laps.floor();
        if amount_of_laps.fract() == 0.0 {
            laps_go_minus -= 1.0;
        }
        // Laptime slower than this may reduce a lap.
        let max_laptime = if laps_go_minus <= 0.0 {
            naive_time_millis(minutes, seconds)
        } else {
            let max_lap_seconds = length_in_seconds / laps_go_minus;
            let max_lap_minutes = (max_lap_seconds / 60.0) as u32;
            naive_time_millis(max_lap_minutes, max_lap_seconds)
        }
        .unwrap_or_default()
        .format("%M:%S%.3f");

        let mut fuel_recommended = fuel_needed;

        let laps_are_close = amount_of_laps.fract() > 0.8 || amount_of_laps.fract() == 0.0;
        if laps_are_close {
            fuel_recommended += consumption
        }

        let fuel_is_close = fuel_recommended.fract() > 0.5 || fuel_recommended.fract() == 0.0;
        if fuel_is_close {
            fuel_recommended += match stint {
                ..=30 => 1.0,
                _ => 2.0,
            }
        }

        let embed = EmbedBuilder::new()
            .title("⛽ Fuel kalkulus")
            .field(EmbedFieldBuilder::new("Minimum", fuel_needed.ceil().to_string()).inline())
            .field(
                EmbedFieldBuilder::new("Recommended", fuel_recommended.ceil().to_string()).inline(),
            )
            .field(
                EmbedFieldBuilder::new(
                    "Laps",
                    format!("{} (*{amount_of_laps:.2}*)", amount_of_laps.ceil()),
                )
                .inline(),
            )
            .field(EmbedFieldBuilder::new(
                "Laptime window for minimum fuel",
                format!("{} - {}", min_laptime, max_laptime),
            ))
            .footer(EmbedFooterBuilder::new(format!(
                "Stint: {stint}, Laptime: {laptime}, Usage: {consumption}",
                stint = NaiveTime::from_hms_opt((stint / 60) as u32, (stint % 60) as u32, 0)
                    .unwrap_or_default(),
                laptime = naive_time_millis(minutes, seconds)
                    .unwrap_or_default()
                    .format("%M:%S%.3f"),
            )))
            .color(0xDB3DBE)
            .build();

        ctx.interaction()
            .update_response(&req.interaction.token)
            .embeds(Some(&[embed]))?
            .send()
            .await?;

        Ok(Response::none())
    }
}

/// Create a `NaiveTime` with `H:M:S.f` from minutes and seconds.
/// This ignores overflow by using remainder.
fn naive_time_millis(minutes: u32, seconds: f64) -> Option<NaiveTime> {
    NaiveTime::from_hms_milli_opt(
        minutes / 60,
        minutes % 60,
        (seconds % 60.0) as u32,
        (seconds.fract() * 1000.0).round() as u32,
    )
}
