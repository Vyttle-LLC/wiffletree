//! The host's side of a pairing exchange on `/pair`. The code is checked only through SPAKE2
//! key confirmation, and the QR code's secret only through the device's proof; each failed
//! check is a wrong try, and the third closes pairing mode.
use super::{Identity, Shared, ToActor, listener::ServerSocket, websocket};
use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use workspace_core::{
    PAIR_METHOD_CODE, PAIR_METHOD_QR, PAIRING_TRIES, PairConfirm, PairReply, PairStart,
    pairing::{self, CodeExchange, Side},
};

pub(super) fn serve(mut socket: ServerSocket, identity: &Identity, shared: &Shared) -> Result<()> {
    let start: PairStart = websocket::receive(&mut socket)?;
    let reply = match exchange(&mut socket, start, identity, shared) {
        Ok(reply) => reply,
        Err(error) => PairReply::Refused {
            reason: format!("{error:#}"),
        },
    };
    websocket::send(&mut socket, &reply)?;
    let _ = socket.close(None);
    let _ = socket.flush();
    Ok(())
}

/// Runs the exchange up to the host's last answer, which is a refusal on any error.
fn exchange(
    socket: &mut ServerSocket,
    start: PairStart,
    identity: &Identity,
    shared: &Shared,
) -> Result<PairReply> {
    let host = &identity.name;
    let by_qr = match start.method.as_str() {
        PAIR_METHOD_CODE => false,
        PAIR_METHOD_QR => true,
        method => anyhow::bail!("{host} does not offer pairing by {method}"),
    };
    let device_name = pairing::device_name(&start.device_name);
    anyhow::ensure!(!device_name.is_empty(), "Name this device");
    let device_key = BASE64
        .decode(&start.device_key)
        .context("Malformed device key")?;
    anyhow::ensure!(
        pairing::is_public_key(&device_key),
        "The device key is not a P-256 key"
    );
    // The SPAKE2 message for `code`, the proof for `qr`.
    let theirs = if by_qr { &start.proof } else { &start.spake };
    let theirs = BASE64
        .decode(theirs.as_deref().unwrap_or_default())
        .context("Malformed pairing message")?;
    let (code, secret, generation) = {
        let mut pairing = shared.open_pairing();
        let mode = pairing
            .as_mut()
            .with_context(|| format!("Pairing is not open on {host}. Turn it on there first."))?;
        anyhow::ensure!(
            !mode.busy,
            "Another device is pairing with {host}. Try again in a moment."
        );
        mode.busy = true;
        (mode.code.clone(), mode.secret.clone(), mode.generation)
    };
    let _release = Release(shared, generation);
    let transcript =
        pairing::transcript(&identity.key.public_key(), &device_key, &device_name, host);
    let (key, given) = match by_qr {
        true => (secret, theirs),
        false => {
            let (spake, ours) = CodeExchange::start(Side::Host, &code)?;
            let key = spake.finish(&theirs)?;
            websocket::send(
                socket,
                &PairReply::Spake {
                    spake: BASE64.encode(ours),
                    host_name: host.clone(),
                },
            )?;
            let confirm: PairConfirm = websocket::receive(socket)?;
            (key, BASE64.decode(&confirm.confirm).unwrap_or_default())
        }
    };
    if !pairing::confirms(&key, Side::Device, &transcript, &given) {
        wrong_try(shared, generation);
        match by_qr {
            true => anyhow::bail!("The QR code did not match"),
            false => anyhow::bail!("The code did not match"),
        }
    }
    // The code and QR code work once: this exchange takes its own pairing mode, still open, or
    // fails.
    {
        let mut pairing = shared.open_pairing();
        anyhow::ensure!(
            pairing
                .as_ref()
                .is_some_and(|mode| mode.generation == generation),
            "Pairing closed on {host} before the code was typed"
        );
        *pairing = None;
    }
    let (reply, stored) = async_channel::bounded(1);
    shared.requests.tell(ToActor::Paired {
        key: device_key,
        name: device_name,
        by_qr,
        reply,
    })?;
    stored
        .recv_blocking()
        .context("The host stopped")?
        .map_err(anyhow::Error::msg)?;
    Ok(PairReply::Paired {
        confirm: BASE64.encode(pairing::confirmation(&key, Side::Host, &transcript)),
        host_id: identity.key.id(),
    })
}

/// Counts a failed confirmation against the pairing mode it was made under; the third closes
/// that mode.
fn wrong_try(shared: &Shared, generation: u64) {
    let mut pairing = shared.pairing();
    let closed = (pairing.as_mut())
        .filter(|mode| mode.generation == generation)
        .is_some_and(|mode| {
            mode.wrong += 1;
            mode.wrong >= PAIRING_TRIES
        });
    if closed {
        *pairing = None;
        drop(pairing);
        let _ = shared.requests.tell(ToActor::Changed);
    }
}

/// Lets another exchange start under the same pairing mode once this one ends, however it ends.
struct Release<'a>(&'a Shared, u64);
impl Drop for Release<'_> {
    fn drop(&mut self) {
        if let Some(mode) = (self.0.pairing().as_mut()).filter(|mode| mode.generation == self.1) {
            mode.busy = false;
        }
    }
}
