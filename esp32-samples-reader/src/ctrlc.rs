use nix::sys::{
    signal::{sigaction, SaFlags, SigAction, SigHandler, Signal::SIGINT},
    signalfd::SigSet,
};
use std::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
};

thread_local! {
    static CTRLC_HANDLED: AtomicBool = AtomicBool::new(false);
}

pub struct CtrlCIgnoredContext {
    inner: PhantomData<()>,
}

impl CtrlCIgnoredContext {
    pub fn has_received_ctrlc(&self) -> bool {
        CTRLC_HANDLED.with(|value| value.load(Ordering::Relaxed))
    }
}

pub struct CtrlCIgnoredOutput<A> {
    pub has_received_ctrlc: bool,
    pub output: A,
}

#[no_mangle]
pub extern "C" fn handle_ignore_sigint(_signal: i32) {
    CTRLC_HANDLED.with(|value| value.store(true, Ordering::Relaxed));
}

fn disable_ctrlc() -> anyhow::Result<SigAction> {
    let mut sigset = SigSet::empty();
    sigset.add(SIGINT);

    let action = SigAction::new(
        SigHandler::Handler(handle_ignore_sigint),
        SaFlags::empty(),
        sigset,
    );
    unsafe { Ok(sigaction(SIGINT, &action)?) }
}

pub fn ignoring_ctrlc<A, F: FnMut(&CtrlCIgnoredContext) -> A>(
    mut f: F,
) -> anyhow::Result<CtrlCIgnoredOutput<A>> {
    CTRLC_HANDLED.with(|value| {
        value.store(false, Ordering::Relaxed);
    });
    let previous_action = disable_ctrlc()?;
    let context = CtrlCIgnoredContext { inner: PhantomData };
    let result = f(&context);
    unsafe {
        sigaction(SIGINT, &previous_action)?;
    }
    Ok(CtrlCIgnoredOutput {
        has_received_ctrlc: context.has_received_ctrlc(),
        output: result,
    })
}
