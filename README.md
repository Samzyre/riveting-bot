# Riveting Bot

This is, the one and only, Riveting bot.
It's built in [Rust][rust-lang] with [Twilight] and runs in a [Docker][docker] container
...or without.

Primarily made for the **Riveting** community discord server. _(And to keep that one guy in check.)_

# How to build and run

The bot will read the discord token from the environment variable `DISCORD_TOKEN`,
which must be set for the bot to connect.

You may use a `.env` file in the project root directory to specify the token
or any other environment variables for the bot.

## Build with Rust

- Have [rust-lang] installed with latest nightly toolchain.
- _a)_ To just build it, run `cargo build` _(for debug build)_ or `cargo build --release`
  _(for optimized build)_.
- _b)_ Or to build and run: `cargo run` or `cargo run --release`.
- _(Optional)_ You can use `cargo` options `--features` and `--no-default-features` to build with
  other features.
- _(Optional)_ You can run the executable directly, once built. By default, found in
  `./target/<build>/`.

#### Example

`cargo build --features=debug`<br>
`cargo run --release --no-default-features --features=admin,bulk-delete`

## Build with Docker

- Have [Docker] installed.

- ### With `docker-compose` _(the easy mode)_

  - To run: `docker compose up -d`. To also rebuild the image before starting, add `--build` to it.
  - To stop the container(s): `docker compose down`.

- ### With base `Dockerfile`

  - To build, run `docker build -t riveting-bot .` in the project root directory.
  - To run the container, run `docker run -d --rm --name riveting-bot --env-file .env riveting-bot`
    (you can use `--env` option instead if you don't have a `.env` file).
    You may want to set up a volume bind with `--mount type=bind,source="$(pwd)"/data,target=/data`.
  - To shutdown the container, run `docker stop riveting-bot`.

  ***

  #### Image features:

  - [`Dockerfile`](Dockerfile): `default`
    _(minimal size, default for docker-compose)_
  - [`Dockerfile.extras`](Dockerfile.extras): `default` + `voice`
    _(built by `docker` github workflow)_

## Dependencies

- `voice` feature requires Opus, it can be built from source if `cmake` is available.
  Additionally, `yt-dlp` is required at runtime. For more information, see
  [songbird dependencies](https://github.com/serenity-rs/songbird?tab=readme-ov-file#dependencies).

# Contributing

- The best place to search docs for the many crates of `twilight` is probably their
  [documentation][twilight-docs].

# Notes

- All of bot's data is located in `./data` folder, which will be created if it doesn't exist yet.
  It will contain logs and configs.
- Any manual changes to configs while the bot is running _may_ be lost.
- To control what is logged to a log file, the bot uses `RUST_LOG` environment variable.
  eg. `RUST_LOG=warn,twilight=info,riveting_bot=debug` which will log `warn` messages,
  `info` for `twilight*`, and `debug` for `riveting_bot` sources.
- Why `twilight` and not `serenity` or something? Because, yes.

[rust-lang]: https://www.rust-lang.org/
[twilight]: https://twilight.rs/
[twilight-docs]: https://api.twilight.rs/twilight/index.html
[docker]: https://www.docker.com/
