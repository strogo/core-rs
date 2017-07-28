//! Dispatch takes messages sent from our wonderful UI and runs the needed core
//! code to generate the response. Essentially, it's the RPC endpoint for core.
//!
//! Each message sent in is in the following format (JSON):
//! 
//!     ["<message id>", "<command>", arg1, arg2, ...]
//!
//! where the arg\* can be any valid JSON object. The Message ID is passed in
//! when responding so the client knows which request we are responding to.

use ::jedi::{self, Value};

use ::error::{TResult, TError};
use ::util;
use ::config;
use ::util::event::Emitter;
use ::turtl::Turtl;
use ::search::Query;
use ::models::user::User;
use ::models::space::Space;
use ::models::board::Board;
use ::models::note::Note;
use ::models::invite::Invite;
use ::models::sync_record::{SyncAction, SyncType};
use ::models::feedback::Feedback;
use ::sync::sync_model;
use ::sync::outgoing::SyncOutgoing;

/// Does our actual message dispatching
fn dispatch(cmd: &String, turtl: &Turtl, data: Value) -> TResult<Value> {
    match cmd.as_ref() {
        "user:login" => {
            let username = jedi::get(&["2"], &data)?;
            let password = jedi::get(&["3"], &data)?;
            turtl.login(username, password)?;
            Ok(jedi::obj())
        },
        "user:join" => {
            let username = jedi::get(&["2"], &data)?;
            let password = jedi::get(&["3"], &data)?;
            turtl.join(username, password)?;
            Ok(jedi::obj())
        },
        "user:logout" => {
            turtl.logout()?;
            util::sleep(1000);
            Ok(jedi::obj())
        },
        "user:delete-account" => {
            turtl.delete_account()?;
            Ok(jedi::obj())
        },
        "app:wipe-user-data" => {
            turtl.wipe_user_data()?;
            Ok(jedi::obj())
        },
        "app:wipe-app-data" => {
            turtl.wipe_app_data()?;
            Ok(jedi::obj())
        },
        "sync:start" => {
            turtl.sync_start()?;
            Ok(jedi::obj())
        },
        "sync:pause" => {
            turtl.sync_pause();
            Ok(jedi::obj())
        },
        "sync:resume" => {
            turtl.sync_resume();
            Ok(jedi::obj())
        },
        "sync:shutdown" => {
            turtl.sync_shutdown(true)?;
            Ok(jedi::obj())
        },
        "sync:delete-item" => {
            let sync_id: String = jedi::get(&["2"], &data)?;
            SyncOutgoing::delete_sync_item(turtl, &sync_id)?;
            Ok(jedi::obj())
        },
        "sync:get-frozen" => {
            let frozen = SyncOutgoing::get_all_frozen(turtl)?;
            Ok(jedi::to_val(&frozen)?)
        },
        "sync:get-pending" => {
            let frozen = SyncOutgoing::get_all_pending(turtl)?;
            Ok(jedi::to_val(&frozen)?)
        },
        "sync:unfreeze-item" => {
            let sync_id: String = jedi::get(&["2"], &data)?;
            SyncOutgoing::kick_frozen_sync(turtl, &sync_id)?;
            Ok(jedi::obj())
        },
        "app:api:set-endpoint" => {
            let endpoint: String = jedi::get(&["2"], &data)?;
            config::set(&["api", "endpoint"], &endpoint)?;
            Ok(jedi::obj())
        },
        "app:shutdown" => {
            turtl.sync_shutdown(false)?;
            turtl.events.trigger("app:shutdown", &jedi::obj());
            Ok(jedi::obj())
        },
        "profile:load" => {
            let profile_guard = turtl.profile.read().unwrap();
            let profile_data = json!({
                "spaces": &profile_guard.spaces,
                "boards": &profile_guard.boards,
            });
            Ok(profile_data)
        },
        "profile:sync:model" => {
            let action: SyncAction = match jedi::get(&["2"], &data) {
                Ok(action) => action,
                Err(e) => return Err(TError::BadValue(format!("dispatch: {} -- bad sync action: {}", cmd, e))),
            };
            let ty: SyncType = jedi::get(&["3"], &data)?;

            match action.clone() {
                SyncAction::Add | SyncAction::Edit => {
                    let val = match ty {
                        SyncType::User => {
                            let mut model: User = jedi::get(&["4"], &data)?;
                            sync_model::save_model(action, turtl, &mut model, false)?
                        }
                        SyncType::Space => {
                            let mut model: Space = jedi::get(&["4"], &data)?;
                            sync_model::save_model(action, turtl, &mut model, false)?
                        }
                        SyncType::Board => {
                            let mut model: Board = jedi::get(&["4"], &data)?;
                            sync_model::save_model(action, turtl, &mut model, false)?
                        }
                        SyncType::Note => {
                            let mut model: Note = jedi::get(&["4"], &data)?;
                            sync_model::save_model(action, turtl, &mut model, false)?
                        }
                        SyncType::Invite => {
                            let mut model: Invite = jedi::get(&["4"], &data)?;
                            sync_model::save_model(action, turtl, &mut model, false)?
                        }
                        SyncType::File => {
                            Value::Null
                        }
                        _ => {
                            return Err(TError::BadValue(format!("dispatch: {} -- cannot direct sync an item of type {:?}", cmd, ty)));
                        }
                    };
                    Ok(val)
                },
                SyncAction::Delete => {
                    let id: String = jedi::get(&["4", "id"], &data)?;
                    match ty {
                        SyncType::User => {
                            sync_model::delete_model::<User>(turtl, &id, false)?;
                        }
                        SyncType::Space => {
                            sync_model::delete_model::<Space>(turtl, &id, false)?;
                        }
                        SyncType::Board => {
                            sync_model::delete_model::<Board>(turtl, &id, false)?;
                        }
                        SyncType::Note => {
                            sync_model::delete_model::<Note>(turtl, &id, false)?;
                        }
                        SyncType::Invite => {
                            sync_model::delete_model::<Invite>(turtl, &id, false)?;
                        }
                        SyncType::File => {
                        }
                        _ => {
                            return Err(TError::BadValue(format!("dispatch: {} -- cannot direct sync an item of type {:?}", cmd, ty)));
                        }
                    }
                    Ok(jedi::obj())
                },
            }
        },
        "profile:get-notes" => {
            let note_ids = jedi::get(&["2"], &data)?;
            let notes: Vec<Note> = turtl.load_notes(&note_ids)?;
            Ok(jedi::to_val(&notes)?)
        },
        "profile:find-notes" => {
            let qry: Query = jedi::get(&["2"], &data)?;
            let search_guard = turtl.search.read().unwrap();
            if search_guard.is_none() {
                return Err(TError::MissingField(format!("dispatch: {} -- turtl is missing `search` object", cmd)));
            }
            let search = search_guard.as_ref().unwrap();
            let note_ids = search.find(&qry)?;
            let notes: Vec<Note> = turtl.load_notes(&note_ids)?;
            Ok(jedi::to_val(&notes)?)
        },
        "profile:get-tags" => {
            let space_id: String = jedi::get(&["2"], &data)?;
            let boards: Vec<String> = jedi::get(&["3"], &data)?;
            let limit: i32 = jedi::get(&["4"], &data)?;
            let search_guard = turtl.search.read().unwrap();
            if search_guard.is_none() {
                return Err(TError::MissingField(format!("dispatch: {} -- turtl is missing `search` object", cmd)));
            }
            let search = search_guard.as_ref().unwrap();
            let tags = search.tags_by_frequency(&space_id, &boards, limit)?;
            Ok(jedi::to_val(&tags)?)
        },
        "feedback:send" => {
            let feedback: Feedback = jedi::get(&["2"], &data)?;
            feedback.send(turtl)?;
            Ok(jedi::obj())
        },
        "ping" => {
            info!("ping!");
            Ok(Value::String(String::from("pong")))
        },
        _ => {
            Err(TError::MissingCommand(cmd.clone()))
        }
    }
}

/// process a message from the messaging system. this is the main communication
/// heart of turtl core.
pub fn process(turtl: &Turtl, msg: &String) -> TResult<()> {
    let data: Value = jedi::parse(msg)?;

    // grab the request id from the data
    let mid: String = match jedi::get(&["0"], &data) {
        Ok(x) => x,
        Err(_) => return Err(TError::MissingField(String::from("missing mid (0)"))),
    };
    // grab the command from the data
    let cmd: String = match jedi::get(&["1"], &data) {
        Ok(x) => x,
        Err(_) => return Err(TError::MissingField(String::from("missing cmd (1)"))),
    };

    info!("dispatch({}): {}", mid, cmd);

    match dispatch(&cmd, turtl.clone(), data) {
        Ok(val) => {
            match turtl.msg_success(&mid, val) {
                Err(e) => error!("dispatch::process() -- problem sending response (mid {}): {}", mid, e),
                _ => {},
            }
        },
        Err(e) => {
            match turtl.msg_error(&mid, &e) {
                Err(e) => error!("dispatch:process() -- problem sending (error) response (mod {}): {}", mid, e),
                _ => {},
            }
        },
    }
    Ok(())
}

