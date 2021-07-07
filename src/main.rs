use dotenv;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::env;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use teloxide::prelude::*;

use teloxide::types::{InputFile, MediaKind, MessageKind};
use tokio_stream::wrappers::UnboundedReceiverStream;

type Timestamp = i32;
type Cxt = UpdateWithCx<AutoSend<Bot>, Message>;
type TgErr<T> = anyhow::Result<T>;

pub struct UserStat {
    latest_voice_message_timestamp: Timestamp,
    has_restricted_voice: bool,
}

lazy_static! {
    pub static ref ALLOWED_VOICE_MESSAGE_DELAY: i32 = 1000 * 60 * 30;
    pub static ref BOT_TOKEN: String =
        dotenv::var("TELOXIDE_TOKEN").expect("TELOXIDE_TOKEN is empty");
    pub static ref MESSAGES_TOTAL: AtomicU64 = AtomicU64::new(0);
    pub static ref USERS_BAN_MAP: Mutex<HashMap<i64, UserStat>> = Mutex::new(HashMap::new());
}

#[tokio::main]
async fn main() {
    run().await;
}

fn get_current_timestamp() -> i32 {
    let since_the_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    since_the_epoch.as_secs() as i32
}

async fn handle_ban(
    message: &Cxt,
    user_id: i64,
    message_id: i32,
    chat_id: i64,
    datetime: &Timestamp,
) {
    match USERS_BAN_MAP.lock().unwrap().get(&user_id) {
        Some(user) => {
            println!("{}", user.has_restricted_voice);
            if !user.has_restricted_voice {
                let current_timestamp = get_current_timestamp();
                let timestamp_diff = current_timestamp - user.latest_voice_message_timestamp;
                let should_be_restricted = timestamp_diff < *ALLOWED_VOICE_MESSAGE_DELAY;
                if should_be_restricted {
                    USERS_BAN_MAP
                        .lock()
                        .unwrap()
                        .get_mut(&user_id)
                        .unwrap()
                        .has_restricted_voice = true;
                } else {
                    USERS_BAN_MAP
                        .lock()
                        .unwrap()
                        .get_mut(&user_id)
                        .unwrap()
                        .latest_voice_message_timestamp = *datetime;
                }
            } else {
                message
                    .requester
                    .delete_message(chat_id, message_id)
                    .await
                    .log_on_error()
                    .await;
            }
        }
        None => {
            USERS_BAN_MAP.lock().unwrap().insert(
                user_id,
                UserStat {
                    latest_voice_message_timestamp: *datetime,
                    has_restricted_voice: false,
                },
            );
        }
    };
}

async fn answer(message: Cxt) -> TgErr<()> {
    let kind = &message.update.kind;
    let chat_id = message.update.chat_id();
    let message_id = message.update.id;
    let audio_reply = InputFile::File(PathBuf::from("reply.ogg"));

    if let MessageKind::Common(msg) = kind {
        if let MediaKind::Voice(_) = &msg.media_kind {
            let user = msg.from.as_ref().unwrap();
            let datetime = &message.update.date;
            let user_id = user.id;

            handle_ban(&message, user_id, message_id, chat_id, datetime).await;

            message
                .requester
                .send_voice(chat_id, audio_reply)
                .reply_to_message_id(message_id)
                .await
                .log_on_error()
                .await;
        }
    }
    Ok(())
}

async fn run() {
    dotenv::dotenv().ok();
    teloxide::enable_logging!();

    println!("BOT STARTED");

    let bot = Bot::from_env().auto_send();
    Dispatcher::new(bot)
        .messages_handler(|rx: DispatcherHandlerRx<AutoSend<Bot>, Message>| {
            UnboundedReceiverStream::new(rx).for_each_concurrent(None, |cx| async move {
                answer(cx).await.log_on_error().await
            })
        })
        .dispatch()
        .await;
}
