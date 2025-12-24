use std::{collections::HashMap, time::Duration};

use async_sqlite::{Pool, rusqlite::params};

use base64::{Engine, prelude::BASE64_STANDARD};
use color_eyre::{Result, eyre::eyre};
use poise::{
    ApplicationContext, Context, CreateReply, execute_modal_on_component_interaction,
    serenity_prelude::{
        self, ComponentInteractionDataKind, CreateActionRow, CreateButton, CreateEmbed,
        CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption, GatewayIntents,
    },
};
use rand::{Rng, RngCore, SeedableRng, seq::SliceRandom};

use tracing::{Level, event};
use tracing_subscriber::util::SubscriberInitExt;
mod app_errs;

struct AppState {
    db: Pool,
}
#[derive(Debug)]
#[allow(unused)]
enum AppErr {
    SerenityErr(serenity_prelude::Error),
    EnvVarError(std::env::VarError),
    ParseIdErr(std::num::ParseIntError),
    DatabaseErr(async_sqlite::Error),

    AdHocErr(color_eyre::eyre::ErrReport),
}
impl std::fmt::Display for AppErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:#?}")
    }
}

type AppContext<'a> = ApplicationContext<'a, AppState, AppErr>;
type AppResult = std::result::Result<(), AppErr>;

const _APP_INTENTS: GatewayIntents = GatewayIntents::non_privileged();

#[poise::command(slash_command)]
async fn info(ctx: AppContext<'_>) -> AppResult {
    let reply_embed = CreateEmbed::new()
        .title("SecretSatan info")
        .description(concat!(
            "* Bot version: ",
            std::env!("CARGO_PKG_VERSION"),
            "\n",
            "* Author: Crinfarr (<@302211105151778826>)\n",
            "* Last build: <t:",
            std::env!("SECSAT_BUILD_TIME"),
            ">\n",
            "* Built on Poise v0.6.1"
        ));
    ctx.send(CreateReply {
        embeds: vec![reply_embed],
        ..Default::default()
    })
    .await?;
    Ok(())
}

#[poise::command(slash_command, subcommands("create", "join"))]
async fn party(_ctx: AppContext<'_>) -> AppResult {
    event!(Level::WARN, "Impossible parent command 'party' was called!");
    Ok(())
}
#[derive(poise::Modal, Debug)]
#[name = "Signup"]
struct JoinPartyForm {
    #[name = "Full name"]
    #[placeholder = "Firstname Lastnameson"]
    #[min_length = 2]
    user_fullname: String,
    #[name = "Hints"]
    #[placeholder = "Anything you want the person matched with you to know"]
    #[paragraph]
    user_hints: String,
}
#[poise::command(slash_command, identifying_name = "join_party", ephemeral)]
async fn join(
    ctx: AppContext<'_>,
    #[description = "The join phrase for the party"] joinphrase: String,
) -> AppResult {
    let rng_seed = {
        let mut v = Vec::default();
        match mnemonic::decode(joinphrase, &mut v) {
            Ok(_) => {}
            Err(_) => {
                ctx.reply("Incorrect join phrase").await?;
                return Ok(());
            }
        };
        if v.len() != 4 {
            ctx.reply("Incorrect join phrase").await?;
            return Ok(());
        }
        u32::from_be_bytes([v[0], v[1], v[2], v[3]])
    };
    let party_id = uuid::Builder::from_random_bytes({
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(rng_seed as u64);
        let mut v: [u8; 16] = [0; 16];
        rng.fill_bytes(&mut v);
        v
    })
    .into_uuid();
    let party_status = ctx
        .data
        .db
        .conn(move |dbc| {
            dbc.query_one(
                &format!("SELECT * FROM party_info WHERE id = \"{party_id}\";"),
                [],
                |row| {
                    if row.get::<&str, i64>("ends_at")? <= chrono::Utc::now().timestamp() {
                        return Ok(Err(row.get::<&str, String>("party_name")?));
                    }
                    Ok(Ok(row.get::<&str, String>("party_name")?))
                },
            )
        })
        .await;
    match party_status {
        Ok(Err(pname)) => {
            ctx.reply(format!("The party {pname} is not accepting signups"))
                .await?;
            return Ok(());
        }
        Ok(Ok(party_name)) => {
            let reply_handle = ctx
                .send(CreateReply {
                    embeds: vec![CreateEmbed::new().title(format!("Join {party_name}?"))],
                    components: Some(vec![CreateActionRow::Buttons(vec![
                        CreateButton::new("join_btn")
                            .label("Join")
                            .style(serenity_prelude::ButtonStyle::Primary),
                        CreateButton::new("cancel_btn")
                            .label("Cancel")
                            .style(serenity_prelude::ButtonStyle::Danger),
                    ])]),
                    ..Default::default()
                })
                .await?;
            let btn_interaction = reply_handle
                .message()
                .await?
                .await_component_interaction(ctx)
                .author_id(ctx.author().id)
                .await;
            match btn_interaction {
                Some(interaction) => match interaction.data.custom_id.as_str() {
                    "join_btn" => {
                        reply_handle
                            .edit(
                                Context::Application(ctx),
                                CreateReply {
                                    content: Some("Processing...".to_owned()),
                                    components: Some(vec![]),
                                    ..Default::default()
                                },
                            )
                            .await?;
                        let form_response =
                            execute_modal_on_component_interaction::<JoinPartyForm>(
                                ctx,
                                interaction,
                                None,
                                Some(Duration::from_mins(10)),
                            )
                            .await?;
                        match form_response {
                            Some(response) => {
                                let (user_name, user_hints) = (
                                    base64::prelude::BASE64_STANDARD.encode(response.user_fullname),
                                    base64::prelude::BASE64_STANDARD.encode(response.user_hints),
                                );
                                let uname_handle = user_name.clone();
                                let uid = ctx.author().id.get();
                                let db_response = ctx.data()
                                        .db
                                        .conn(move |dbc| {
                                            dbc.execute(
                                                &format!(
                                                    "INSERT INTO \"{party_id}\" (uid, name, hint) VALUES (?1, ?2, ?3);"
                                                ),
                                                params![uid, uname_handle, user_hints],
                                            )
                                        })
                                        .await;
                                if let Err(e) = db_response {
                                    match e {
                                        async_sqlite::Error::Rusqlite(rusqlite_err) => {
                                            if let Some(errcode) = rusqlite_err.sqlite_error_code()
                                            {
                                                match errcode {
                                                    async_sqlite::rusqlite::ErrorCode::ConstraintViolation => {
                                                        reply_handle.edit(
                                                            Context::Application(ctx),
                                                            CreateReply {
                                                                content: Some("Failed to join: You are already in this party!".to_owned()),
                                                                components: Some(vec![]),
                                                                ..Default::default()
                                                        }).await?;
                                                        return Ok(());
                                                    },
                                                    _ => {}
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                reply_handle
                                    .edit(
                                        Context::Application(ctx),
                                        CreateReply {
                                            content: Some(format!("You joined {party_name}.")),
                                            components: Some(vec![]),
                                            ..Default::default()
                                        },
                                    )
                                    .await?;
                                return Ok(());
                            }
                            None => {
                                reply_handle
                                    .edit(
                                        Context::Application(ctx),
                                        CreateReply {
                                            content: Some(
                                                "Join form timed out or was cancelled".to_owned(),
                                            ),
                                            components: Some(vec![]),
                                            ..Default::default()
                                        },
                                    )
                                    .await?;
                                return Ok(());
                            }
                        }
                    }
                    "cancel_btn" => {
                        reply_handle
                            .edit(
                                Context::Application(ctx),
                                CreateReply {
                                    content: Some("Join cancelled".to_owned()),
                                    components: Some(vec![]),
                                    ..Default::default()
                                },
                            )
                            .await?;
                        return Ok(());
                    }
                    _ => unreachable!("undefined button"),
                },
                None => {
                    ctx.say("Join command timed out, please try again").await?;
                }
            }
        }
        Err(_e) => {
            ctx.reply(format!("No party exists with that join phrase!"))
                .await?;
            return Ok(());
        }
    };
    Ok(())
}

#[poise::command(slash_command, identifying_name = "create_party", ephemeral)]
async fn create(
    ctx: AppContext<'_>,
    #[description = "How long to allow users to join this party"] signup_duration: String,
    #[description = "The public name of this party"] party_name: String,
) -> AppResult {
    let rng_seed = rand::rng().random::<u32>();
    let seedphrase = mnemonic::to_string(&rng_seed.to_be_bytes());
    let id = uuid::Builder::from_random_bytes({
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(rng_seed as u64);
        let mut v: [u8; 16] = [0; 16];
        rng.fill_bytes(&mut v);
        v
    })
    .into_uuid();

    let signup_duration = duration_str::parse(&signup_duration).map_err(|e| eyre!("{e}"))?;

    ctx.data()
        .db
        .conn(move |dbc| {
            dbc.execute(
                &format!(
                    "CREATE TABLE \"{id}\" (
                        uid TEXT NOT NULL UNIQUE,
                        name TEXT NOT NULL,
                        hint TEXT NOT NULL
                    );"
                ),
                [],
            )
        })
        .await?;
    let author_id_handle = ctx.author().id.get();
    ctx.data()
        .db
        .conn(move |dbc| {
            let now = chrono::Utc::now();
            match dbc.execute(&format!(
            "INSERT INTO party_info (id, admin_id, party_name, started_at, ends_at) VALUES (?1, ?2, ?3, ?4, ?5)"
            ), params![
                id.to_string(),
                author_id_handle,
                party_name,
                now.timestamp(),
                (now + signup_duration).timestamp()
            ]) {
                Ok(_) => {Ok(())},
                Err(e) => {event!(Level::WARN, "SQL error: {e}"); Err(e)}
            }
        })
        .await?;
    event!(
        Level::INFO,
        "Created a party with the seed phrase {seedphrase} and the uuid {id}"
    );
    ctx.reply(format!("Created a new party with the join phrase `{seedphrase}`. Don't forget to join your own party!")).await?;
    let party_id = id.clone();
    let dbhandle = ctx.data.db.clone();
    tokio::spawn(async move {
        tokio::time::sleep_until(tokio::time::Instant::now() + signup_duration).await;
        let party_id_a = party_id.clone();
        let mut signed_up = dbhandle
            .conn(move |dbc| {
                #[derive(Clone)]
                struct UserResponse {
                    pub uid: u64,
                    pub name: String,
                    pub hint: String,
                }
                let mut query = dbc.prepare(&format!("SELECT * FROM \"{party_id_a}\";"))?;

                let responses = query.query_map([], |row| {
                    Ok(UserResponse {
                        uid: row.get::<_, u64>("uid")?,
                        name: row.get::<_, String>("name")?,
                        hint: row.get::<_, String>("hint")?,
                    })
                })?;
                let mut ovec = Vec::default();
                for user_response in responses {
                    ovec.push(user_response?);
                }
                ovec.shuffle(&mut rand::rng());
                Ok(ovec)
            })
            .await?;
        event!(Level::INFO, "Party with id {} completed", party_id);
        let givers = {
            let mut s = signed_up
                .iter()
                .map(|user| user.uid.clone())
                .collect::<Vec<_>>();
            let mut no_dupes = true;
            loop {
                s.shuffle(&mut rand::rng());
                for i in 0..s.len() {
                    if s[i] == signed_up[i].uid {
                        event!(Level::INFO, "Shuffle collision detected, rerandomizing");
                        no_dupes = false;
                        break;
                    }
                }
                if no_dupes {
                    break;
                }
            }
            s.reverse();
            s
        };
        let party_id_b = party_id.clone();
        dbhandle
            .conn(move |dbc| {
                dbc.execute(
                    &format!(
                        "CREATE TABLE \"{party_id_b}-matches\" (
                            giver_id integer not null unique,
                            receiver_id integer not null unique,
                            receiver_name text not null,
                            receiver_hint text not null
                        );"
                    ),
                    [],
                )?;
                for giver_id in givers {
                    let receiver = signed_up.pop().unwrap();
                    dbc.execute(
                        &format!(
                            "INSERT INTO \"{party_id_b}-matches\"
                            giver_id, receiver_id, receiver_name, receiver_hint
                        VALUES
                           (?1,       ?2,          ?3,            ?4)"
                        ),
                        params![giver_id, receiver.uid, receiver.name, receiver.hint],
                    )?;
                }
                dbc.execute(
                    "UPDATE TABLE party_info SET matches_made = true WHERE party_id = ?1",
                    [party_id_b.to_string()],
                )?;
                Ok(())
            })
            .await?;
        AppResult::Ok(())
    });
    Ok(())
}
#[poise::command(slash_command, ephemeral)]
async fn get_my_target(ctx: AppContext<'_>) -> AppResult {
    let uid_handle = ctx.author().id.get().clone();
    let user_parties = ctx
        .data
        .db
        .conn(move |dbc| {
            let mut query = dbc.prepare("SELECT id FROM party_info")?;
            let parties = {
                let mut ov = Vec::new();
                let rows = query.query_map([], |info| Ok(info.get::<_, String>("id")?))?;
                for pid in rows {
                    ov.push(pid?);
                }
                ov
            };
            let mut joined_parties = vec![];
            for party in parties {
                if dbc
                    .query_one(
                        &format!("SELECT uid FROM \"{party}\" where uid = ?1;"),
                        [uid_handle],
                        |row| {
                            row.get::<_, String>("uid")?;
                            Ok(())
                        },
                    )
                    .is_ok()
                {
                    joined_parties.push(party);
                }
            }
            Ok(joined_parties)
        })
        .await?;
    let mut responses = HashMap::<String, CreateReply>::default();
    for party_id in user_parties {
        let party_id_handle = party_id.clone();
        let party_data = ctx
            .data
            .db
            .conn(move |dbc| {
                struct PartyData {
                    party_name: String,
                    ends_at: i64,
                }
                Ok(dbc.query_one(
                    "SELECT party_name, ends_at FROM party_info WHERE id = ?1",
                    [party_id_handle],
                    |row| {
                        Ok(PartyData {
                            party_name: row.get::<_, String>("party_name")?,
                            ends_at: row.get::<_, i64>("ends_at")?,
                        })
                    },
                )?)
            })
            .await?;
        if party_data.ends_at > chrono::Utc::now().timestamp() {
            responses.insert(
                party_data.party_name.clone(),
                CreateReply {
                    embeds: vec![CreateEmbed::new().title(party_data.party_name).description(
                        format!(
                            "This party's drawing will occur <t:{}:R>\n.",
                            party_data.ends_at
                        ),
                    )],
                    components: Some(vec![]),
                    ..Default::default()
                },
            );
        } else {
            let uid_handle_b = ctx.author().id.get();
            let party_id_handle_b = party_id.clone();
            let user_match = ctx
                .data
                .db
                .conn(move |dbc| {
                    struct MatchData {
                        name: String,
                        hint: String,
                    }
                    Ok(dbc.query_one(
                        &format!(
                            "SELECT * FROM \"{party_id_handle_b}-matches\" WHERE giver_id = ?1"
                        ),
                        [uid_handle_b],
                        |row| {
                            Ok(MatchData {
                                name: row.get::<_, String>("name")?,
                                hint: row.get::<_, String>("hint")?,
                            })
                        },
                    )?)
                })
                .await?;
            responses.insert(
                party_data.party_name.clone(),
                CreateReply {
                    embeds: vec![CreateEmbed::new().title(party_data.party_name).description(format!(
                        "You have been matched with {}.\n They wanted you to know this:```\n{}\n```",
                        String::from_utf8(BASE64_STANDARD.decode(user_match.name).unwrap()).unwrap(),
                        String::from_utf8(BASE64_STANDARD.decode(user_match.hint).unwrap()).unwrap()
                    ))],
                    components: Some(vec![]),
                    ..Default::default()
                },
            );
        }
    }
    let menu_options = responses
        .keys()
        .map(|party_name| CreateSelectMenuOption::new(party_name, party_name))
        .collect::<Vec<_>>();
    let reply_handle = ctx
        .send(CreateReply {
            content: Some("Select a party".to_owned()),
            components: Some(vec![CreateActionRow::SelectMenu(
                CreateSelectMenu::new(
                    "party_selector",
                    CreateSelectMenuKind::String {
                        options: menu_options,
                    },
                )
                .min_values(1)
                .max_values(1),
            )]),
            ..Default::default()
        })
        .await?;
    let catch_interaction = reply_handle
        .message()
        .await?
        .await_component_interaction(ctx)
        .author_id(ctx.author().id)
        .await;
    match catch_interaction {
        Some(interaction) => match interaction.data.kind {
            ComponentInteractionDataKind::StringSelect { values } => {
                let party_name = values.get(0).unwrap();
                reply_handle
                    .edit(
                        Context::Application(ctx),
                        responses.get(party_name).unwrap().to_owned(),
                    )
                    .await?;
            }
            _ => unreachable!("Invalid interaction data response"),
        },
        None => {
            reply_handle
                .edit(
                    Context::Application(ctx),
                    CreateReply {
                        content: Some("Selection timed out, try again.".to_owned()),
                        components: Some(vec![]),
                        ..Default::default()
                    },
                )
                .await?;
        }
    }
    Ok(())
}

#[poise::command(slash_command, ephemeral)]
async fn ping(ctx: AppContext<'_>) -> AppResult {
    ctx.reply("Pong!").await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish()
        .init();
    event!(Level::INFO, "Reading .env");
    dotenv::dotenv()?;
    let token = std::env::var("DISCORD_TOKEN")?;

    event!(Level::INFO, "Setting up database");
    let db_connection = async_sqlite::PoolBuilder::new()
        .path("live_data/secret_satan.db")
        .journal_mode(async_sqlite::JournalMode::Wal)
        .open()
        .await?;
    db_connection
        .conn(|dbc| {
            dbc.execute(
                "CREATE TABLE IF NOT EXISTS party_info  (
                    id text not null,
                    admin_id integer not null,
                    party_name text not null,
                    started_at integer not null,
                    ends_at integer not null,
                    matches_made bool not null default true
                );",
                [],
            )
        })
        .await?;
    let mut timer_pool = tokio::task::JoinSet::<Result<()>>::new();
    struct PartyInfo {
        pub ends_at: i64,
        pub id: String,
    }
    let pending_parties = db_connection
        .conn(|dbc| {
            let mut statement = dbc.prepare("SELECT id, ends_at FROM party_info;")?;
            let rows = statement.query_map([], |row| {
                Ok(PartyInfo {
                    id: row.get::<&str, String>("id")?.clone(),
                    ends_at: row.get::<&str, i64>("ends_at")?.clone(),
                })
            })?;
            let mut ovec = Vec::<PartyInfo>::default();
            for line in rows {
                let l = line?;
                if l.ends_at < chrono::Utc::now().timestamp() {
                    event!(Level::INFO, "{} already completed", l.id);
                    continue;
                }
                ovec.push(l);
            }
            Ok(ovec)
        })
        .await?;
    for party in pending_parties {
        event!(Level::INFO, "Spawning timer for {}", party.id);
        let db_handle = db_connection.clone();
        timer_pool.spawn(async move {
            tokio::time::sleep_until(
                tokio::time::Instant::now()
                    + chrono::DateTime::from_timestamp_secs(party.ends_at)
                        .ok_or(eyre!("Failed to parse party end time"))?
                        .signed_duration_since(chrono::Utc::now())
                        .to_std()?,
            )
            .await;

            let party_id_a = party.id.clone();
            let mut signed_up = db_handle
                .conn(move |dbc| {
                    #[derive(Clone)]
                    struct UserResponse {
                        pub uid: u64,
                        pub name: String,
                        pub hint: String,
                    }
                    let mut query = dbc.prepare(&format!("SELECT * FROM \"{party_id_a}\";"))?;

                    let responses = query.query_map([], |row| {
                        Ok(UserResponse {
                            uid: row.get::<_, u64>("uid")?,
                            name: row.get::<_, String>("name")?,
                            hint: row.get::<_, String>("hint")?,
                        })
                    })?;
                    let mut ovec = Vec::default();
                    for user_response in responses {
                        ovec.push(user_response?);
                    }
                    ovec.shuffle(&mut rand::rng());
                    Ok(ovec)
                })
                .await?;
            event!(Level::INFO, "Party with id {} completed", party.id);
            let givers = {
                let mut s = signed_up
                    .iter()
                    .map(|user| user.uid.clone())
                    .collect::<Vec<_>>();
                let mut no_dupes = true;
                loop {
                    s.shuffle(&mut rand::rng());
                    for i in 0..s.len() {
                        if s[i] == signed_up[i].uid {
                            event!(Level::INFO, "Shuffle collision detected, rerandomizing");
                            no_dupes = false;
                            break;
                        }
                    }
                    if no_dupes {
                        break;
                    }
                }
                s.reverse();
                s
            };
            let party_id_b = party.id.clone();
            db_handle
                .conn(move |dbc| {
                    dbc.execute(
                        &format!(
                            "CREATE TABLE \"{party_id_b}-matches\" (
                                giver_id integer not null unique,
                                receiver_id integer not null unique,
                                receiver_name text not null,
                                receiver_hint text not null
                            );"
                        ),
                        [],
                    )?;
                    for giver_id in givers {
                        let receiver = signed_up.pop().unwrap();
                        dbc.execute(
                            &format!(
                                "INSERT INTO \"{party_id_b}-matches\"
                                giver_id, receiver_id, receiver_name, receiver_hint
                            VALUES
                               (?1,       ?2,          ?3,            ?4)"
                            ),
                            params![giver_id, receiver.uid, receiver.name, receiver.hint],
                        )?;
                    }
                    dbc.execute(
                        "UPDATE TABLE party_info SET matches_made = true WHERE party_id = ?1",
                        [party_id_b],
                    )?;
                    Ok(())
                })
                .await?;
            Ok(())
        });
    }

    event!(Level::INFO, "Setting up bot");
    let app_framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![party(), ping(), info(), get_my_target()],
            ..Default::default()
        })
        .setup(|ctx, _ready, fw| {
            Box::pin(async move {
                if std::env::var("REGISTER_GLOBAL")? == "true" {
                    event!(Level::INFO, "Registering commands globally");
                    poise::builtins::register_globally(ctx.http.clone(), &fw.options().commands)
                        .await?;
                    event!(Level::INFO, "Commands registered");
                } else {
                    event!(Level::INFO, "Registering commands in native guild");
                    poise::builtins::register_in_guild(
                        ctx.http.clone(),
                        &fw.options().commands,
                        std::env::var("NATIVE_GUILD")?
                            .parse::<poise::serenity_prelude::GuildId>()?,
                    )
                    .await?;
                    event!(Level::INFO, "Commands registered");
                }
                Ok(AppState {
                    db: db_connection.clone(),
                })
            })
        })
        .build();
    event!(Level::INFO, "App constructed successfully");
    let mut app_client = poise::serenity_prelude::ClientBuilder::new(token, _APP_INTENTS)
        .framework(app_framework)
        .await?;
    event!(Level::INFO, "Starting...");
    app_client.start().await?;
    Ok(())
}
