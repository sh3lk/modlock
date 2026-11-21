//! host carries calls across the boundary between the mod and the game: the
//! mod's calls out through modlock.host_call, and the game's events in
//! through the modlock_event export. The generated module and the run! macro
//! call it; a mod has no need to.

use std::any::Any;
use std::cell::Cell;

use crate::runtime::Refusal;
use crate::wire::{self, Error, Reader};
use crate::{Events, Mod};

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "modlock")]
extern "C" {
    /// host_call hands the host one encoded Call and returns the length of
    /// the encoded Reply, which host_read then copies out.
    fn host_call(data: *const u8, size: u32) -> u32;

    /// host_read copies the host's pending message, a Call or a Reply, into
    /// data, which holds exactly the length the host announced.
    fn host_read(data: *mut u8, size: u32);
}

thread_local! {
    /// The buffers a call encodes into and reads its reply from. A call takes
    /// them and puts them back, so the next one allocates nothing.
    static BUFFERS: Cell<(Vec<u8>, Vec<u8>)> = const { Cell::new((Vec::new(), Vec::new())) };

    /// The mod, between events.
    static MOD: Cell<Option<Box<dyn Any>>> = const { Cell::new(None) };

    /// The event being served and the reply to the last one, which the host
    /// reads after modlock_event returns.
    static EVENT: Cell<(Vec<u8>, Vec<u8>)> = const { Cell::new((Vec::new(), Vec::new())) };
}

/// exchange hands the host an encoded Call and reads its encoded Reply into
/// reply.
#[cfg(target_arch = "wasm32")]
fn exchange(call: &[u8], reply: &mut Vec<u8>) {
    reply.clear();
    // The host writes exactly the length it announced into the space
    // reserved for it.
    unsafe {
        let size = host_call(call.as_ptr(), call.len() as u32) as usize;
        reply.reserve(size);
        if size != 0 {
            host_read(reply.as_mut_ptr(), size as u32);
        }
        reply.set_len(size);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn exchange(call: &[u8], reply: &mut Vec<u8>) {
    reply.clear();
    fake::exchange(call, reply);
}

/// call calls one Host method. request appends the method's encoded request,
/// and response reads its result from the encoded response. It returns None
/// when the call failed, which the host logs, or when the response is not
/// what the schema says.
pub fn call<R>(
    method: &str,
    request: impl FnOnce(&mut Vec<u8>),
    response: impl FnOnce(&mut Reader) -> Result<R, Error>,
) -> Option<R> {
    let (mut out, mut reply) = BUFFERS.take();
    out.clear();
    wire::string(&mut out, 1, method);
    wire::nested(&mut out, 2, request);
    exchange(&out, &mut reply);
    let result = match payload(&reply) {
        Ok(Some(payload)) => response(&mut Reader::new(payload)).ok(),
        _ => None,
    };
    BUFFERS.set((out, reply));
    result
}

/// send calls one Host method that answers nothing and returns whether it
/// succeeded.
pub fn send(method: &str, request: impl FnOnce(&mut Vec<u8>)) -> bool {
    call(method, request, |_| Ok(())).is_some()
}

/// payload returns the response an encoded Reply carries, or None for a
/// Reply that carries an error.
fn payload(reply: &[u8]) -> Result<Option<&[u8]>, Error> {
    let mut response: &[u8] = &[];
    let mut r = Reader::new(reply);
    while let Some((number, kind)) = r.field()? {
        match number {
            1 if !r.bytes(kind)?.is_empty() => return Ok(None),
            2 => response = r.bytes(kind)?,
            1 => {}
            _ => r.skip(kind)?,
        }
    }
    Ok(Some(response))
}

/// serve runs the encoded Call in data on the mod, which make makes on the
/// first event, and returns the encoded Reply. events holds the mod's
/// handlers of the events the game sends on request.
pub fn serve<M: Mod + 'static>(
    data: &[u8],
    reply: &mut Vec<u8>,
    make: impl FnOnce() -> M,
    events: &Events<M>,
) {
    reply.clear();

    // Read the method and its request out of the Call.
    let mut method = "";
    let mut request: &[u8] = &[];
    let mut r = Reader::new(data);
    let read = (|| {
        while let Some((number, kind)) = r.field()? {
            match number {
                1 => {
                    method = std::str::from_utf8(r.bytes(kind)?)
                        .map_err(|_| Error("a string is not UTF-8"))?
                }
                2 => request = r.bytes(kind)?,
                _ => r.skip(kind)?,
            }
        }
        Ok(())
    })();
    if let Err(Error(why)) = read {
        wire::string(reply, 1, why);
        return;
    }

    // Take the mod for the length of the event, so an event the game
    // delivers inside one of its calls finds no mod rather than a second
    // reference to it.
    let Some(held) = MOD.take().or_else(|| Some(Box::new(make()))) else {
        return;
    };
    let mut held = match held.downcast::<M>() {
        Ok(held) => held,
        Err(other) => {
            MOD.set(Some(other));
            wire::string(reply, 1, "the module holds a mod of another type");
            return;
        }
    };
    let mut response = Vec::new();
    let served = crate::runtime::deliver(&mut *held, events, method, request, &mut response);
    MOD.set(Some(held));

    // Answer with the response, or with why there is none.
    match served {
        Ok(true) if response.is_empty() => {}
        Ok(true) => wire::bytes(reply, 2, &response),
        Ok(false) => wire::string(reply, 1, &format!("the mod has no method {method}")),
        Err(Refusal(why)) => wire::string(reply, 1, &why),
    }
}

/// event serves the encoded Call of size bytes the host holds and returns the
/// encoded Reply's address in the high 32 bits and its length in the low 32
/// bits, or zero for an empty Reply. The run! macro exports it as
/// modlock_event.
#[cfg(target_arch = "wasm32")]
pub fn event<M: Mod + 'static>(size: u32, make: impl FnOnce() -> M, events: &Events<M>) -> u64 {
    let (mut data, mut reply) = EVENT.take();
    data.clear();
    data.reserve(size as usize);
    // The host writes exactly the length it announced into the space
    // reserved for it.
    unsafe {
        if size != 0 {
            host_read(data.as_mut_ptr(), size);
        }
        data.set_len(size as usize);
    }
    serve(&data, &mut reply, make, events);
    let packed = match reply.len() {
        0 => 0,
        length => (reply.as_ptr() as u64) << 32 | length as u64,
    };
    EVENT.set((data, reply));
    packed
}

/// fake stands for the game outside a module, so the library's tests run on
/// the machine that builds them.
#[cfg(not(target_arch = "wasm32"))]
pub mod fake {
    use std::cell::RefCell;

    use crate::wire::{self, Reader};

    /// Answer answers one call: the method's name and encoded request in, the
    /// encoded response or an error out.
    type Answer = Box<dyn FnMut(&str, &[u8]) -> Result<Vec<u8>, String>>;

    thread_local! {
        static HOST: RefCell<Answer> = RefCell::new(Box::new(|_, _| Ok(Vec::new())));
    }

    /// answer makes host answer every call the mod makes on this thread.
    pub fn answer(host: impl FnMut(&str, &[u8]) -> Result<Vec<u8>, String> + 'static) {
        HOST.with(|held| *held.borrow_mut() = Box::new(host));
    }

    pub(super) fn exchange(call: &[u8], reply: &mut Vec<u8>) {
        let mut method = String::new();
        let mut request: &[u8] = &[];
        let mut r = Reader::new(call);
        while let Some((number, kind)) = r.field().expect("the library encodes a Call") {
            match number {
                1 => method = r.string(kind).expect("the library encodes a Call"),
                2 => request = r.bytes(kind).expect("the library encodes a Call"),
                _ => panic!("a Call has no field {number}"),
            }
        }
        match HOST.with(|host| host.borrow_mut()(&method, request)) {
            Ok(response) if response.is_empty() => {}
            Ok(response) => wire::bytes(reply, 2, &response),
            Err(error) => wire::string(reply, 1, &error),
        }
    }
}
