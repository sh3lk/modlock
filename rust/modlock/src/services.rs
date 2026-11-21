//! services carries calls between the mod and the services of the host: the
//! ones the mod serves, which the host calls, and the ones the host
//! provides, which the mod calls and hears the answer of in a later frame.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use crate::generated::{
    start_service_call, ServiceCall, ServiceFailure, ServiceRepliedEvent, ServiceReply,
};
use crate::runtime::{soon, Refusal};

/// Service answers the calls to one service the mod serves.
type Service = Rc<dyn Fn(&mut dyn Any, &str, &[u8]) -> Result<Vec<u8>, String>>;

/// Done ends one started call on the mod.
pub(crate) type Done = Box<dyn FnOnce(&mut dyn Any, Result<Vec<u8>, ServiceError>)>;

#[derive(Default)]
struct State {
    /// served maps each service the mod serves to its handler.
    served: HashMap<String, Service>,
    /// waiting maps each started call's number to what its end runs.
    waiting: HashMap<u64, Done>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// ServiceError ends a started service call without an answer: failure says
/// why and message describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceError {
    pub failure: ServiceFailure,
    pub message: String,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ServiceError {}

/// serve answers the host's calls to service with handler: a method of the
/// mod that takes the called method's name and payload and returns the
/// answer, or the error the caller gets. Registering a service again replaces
/// its handler.
pub fn serve<M: 'static>(
    service: &str,
    handler: impl Fn(&mut M, &str, &[u8]) -> Result<Vec<u8>, String> + 'static,
) {
    let erased: Service = Rc::new(move |m, method, payload| match m.downcast_mut::<M>() {
        Some(m) => handler(m, method, payload),
        None => Err("the module holds a mod of another type".to_owned()),
    });
    STATE.with(|state| state.borrow_mut().served.insert(service.to_owned(), erased));
}

/// call_service_later calls method of a service the host provides and returns
/// at once; done runs on the mod in a later frame with the answer, or with a
/// ServiceError when the service refused, the call waited past timeout or
/// nobody provides the service. A zero timeout waits the host's longest, 30
/// seconds. done runs once while the mod runs; calls still waiting when the
/// mod stops end with it.
pub fn call_service_later<M: 'static>(
    service: &str,
    method: &str,
    payload: &[u8],
    timeout: Duration,
    done: impl FnOnce(&mut M, Result<Vec<u8>, ServiceError>) + 'static,
) {
    let erased: Done = Box::new(move |m, result| {
        if let Some(m) = m.downcast_mut::<M>() {
            done(m, result);
        }
    });
    call_later(service, method, payload, timeout, erased);
}

/// call_later is call_service_later for the library itself, whose done
/// takes the mod whatever its type.
pub(crate) fn call_later(
    service: &str,
    method: &str,
    payload: &[u8],
    timeout: Duration,
    done: Done,
) {
    // A call the host refuses to start ends in the next frame as well, so
    // done never runs before this returns.
    let milliseconds = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    let Some(call) = start_service_call(service, method, payload, milliseconds) else {
        let error = ServiceError {
            failure: ServiceFailure::Unavailable,
            message: format!("the host did not start the call to {service}"),
        };
        soon(move |m| done(m, Err(error)));
        return;
    };
    STATE.with(|state| state.borrow_mut().waiting.insert(call, done));
}

/// answer runs the handler of the called service on the mod.
pub(crate) fn answer(m: &mut dyn Any, event: &ServiceCall) -> Result<ServiceReply, Refusal> {
    let handler = STATE.with(|state| state.borrow().served.get(&event.service).cloned());
    let Some(handler) = handler else {
        return Err(Refusal(format!(
            "the mod serves no service {}",
            event.service
        )));
    };
    let payload = handler(m, &event.method, &event.payload).map_err(Refusal)?;
    Ok(ServiceReply { payload })
}

/// replied ends the started call event names, once. A number the mod no
/// longer waits for is ignored.
pub(crate) fn replied(m: &mut dyn Any, event: ServiceRepliedEvent) {
    let Some(done) = STATE.with(|state| state.borrow_mut().waiting.remove(&event.call)) else {
        return;
    };
    if event.failure != ServiceFailure::None {
        let error = ServiceError {
            failure: event.failure,
            message: event.error,
        };
        done(m, Err(error));
        return;
    }
    done(m, Ok(event.payload));
}
