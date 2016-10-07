extern crate carrier;
extern crate crossbeam;
extern crate crypto as rust_crypto;
extern crate dumpy;
extern crate fern;
extern crate futures;
extern crate futures_cpupool;
extern crate gcrypt;
extern crate hyper;
extern crate jedi;
#[macro_use]
extern crate lazy_static;
extern crate libc;
#[macro_use]
extern crate log;
#[macro_use]
extern crate quick_error;
extern crate rusqlite;
extern crate rustc_serialize as serialize;
extern crate serde;
extern crate serde_json;
extern crate serde_yaml;
extern crate time;

#[macro_use]
mod error;
mod config;
#[macro_use]
mod util;
mod messaging;
mod api;
mod crypto;
#[macro_use]
mod models;
mod storage;
mod dispatch;
mod turtl;

use ::std::thread;
use ::std::sync::Arc;
use ::std::fs;
use ::std::io::ErrorKind;

use ::crossbeam::sync::MsQueue;
use ::jedi::Value;

use ::error::{TError, TResult};
use ::util::event::Emitter;
use ::util::stopper::Stopper;
use ::util::thredder::Pipeline;

/// Init any state/logging/etc the app needs
pub fn init() -> TResult<()> {
    match util::logger::setup_logger() {
        Ok(..) => Ok(()),
        Err(e) => Err(toterr!(e)),
    }
}

lazy_static!{
    static ref RUN: Stopper = Stopper::new();
}

/// Stop all threads and close down Turtl
pub fn stop(tx: Pipeline) {
    (*RUN).set(false);
    tx.push(Box::new(move |_| {}));
}

struct Config {
    data_folder: String,
    dumpy_schema: Value,
    api_endpoint: String,
}

/// This takes a JSON-encoded object, and parses out the values we care about
/// into a `Config` struct which can be used to configure various parts of the
/// app.
fn process_config(config_str: String) -> Config {
    let runtime_config: Value = match jedi::parse(&config_str) {
        Ok(x) => x,
        Err(_) => jedi::obj(),
    };
    let data_folder: String = match jedi::get(&["data_folder"], &runtime_config) {
        Ok(x) => x,
        Err(_) => String::from("/tmp/turtl.sql"),
    };
    let dumpy_schema: Value = match jedi::get(&["schema"], &runtime_config) {
        Ok(x) => x,
        Err(_) => jedi::obj(),
    };
    let api_endpoint: String = match jedi::get(&["api", "endpoint"], &runtime_config) {
        Ok(x) => x,
        Err(_) => match config::get(&["api", "endpoint"]) {
            Ok(x) => x,
            Err(_) => String::from("https://api.turtlapp.com/v2"),
        },
    };
    Config {
        data_folder: data_folder,
        dumpy_schema: dumpy_schema,
        api_endpoint: api_endpoint,
    }
}

/// Start our app...spawns all our worker/helper threads, including our comm
/// system that listens for external messages.
pub fn start(config_str: String) -> thread::JoinHandle<()> {
    (*RUN).set(true);
    thread::Builder::new().name(String::from("turtl-main")).spawn(move || {
        // load our ocnfiguration
        let runtime_config: Config = process_config(config_str);

        match fs::create_dir(&runtime_config.data_folder[..]) {
            Ok(()) => {
                info!("main::start() -- created data folder: {}", runtime_config.data_folder);
            },
            Err(e) => {
                match e.kind() {
                    ErrorKind::AlreadyExists => (),
                    _ => {
                        error!("main::start() -- error creating data folder: {:?}", e.kind());
                        return;
                    }
                }
            }
        }

        let queue_main = Arc::new(MsQueue::new());

        // start our messaging thread
        let (tx_msg, handle) = messaging::start(queue_main.clone());

        // create our turtl object
        let turtl = match turtl::Turtl::new_wrap(queue_main.clone(), tx_msg, &runtime_config.data_folder, runtime_config.dumpy_schema.clone()) {
            Ok(x) => x,
            Err(err) => {
                error!("main::start() -- error creating Turtl object: {}", err);
                return;
            }
        };

        // bind turtl.events "app:shutdown" to close everything
        {
            let ref mut events = turtl.write().unwrap().events;
            let tx_main_shutdown = queue_main.clone();
            events.bind("app:shutdown", move |_| {
                stop(tx_main_shutdown.clone());
            }, "app:shutdown");
        }

        // set our api endpoint
        turtl.write().unwrap().api.set_endpoint(&runtime_config.api_endpoint);

        // run our main loop. all threads pipe their data/responses into this
        // loop, meaning <main> only has to check one place to grab messages.
        // this creates an event loop of sorts, without all the grossness.
        info!("main::start() -- main loop");
        while (*RUN).running() {
            debug!("turtl: main thread message loop");
            let handler = queue_main.pop();
            handler.call_box(turtl.clone());
        }
        info!("main::start() -- shutting down");
        turtl.write().unwrap().shutdown();
        match handle.join() {
            Ok(..) => {},
            Err(e) => error!("main: problem joining message thread: {:?}", e),
        }
    }).unwrap()
}

/// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
/// TODO: when calling this from C, handle all panics, or get rid of panics.
/// see https://doc.rust-lang.org/std/panic/fn.catch_unwind.html
/// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
fn main() {
    init().unwrap();
    let config = String::from(r#"{"data_folder":"d:/tmp/turtl/","api":{"endpoint":"http://api.turtl.dev:8181"}}"#);
    start(config).join().unwrap();
}

