//! Explicit assessment capability and the checked occurrence writer. Unknown arguments and
//! incompatible protocols fail before connecting; the store owns every authorization/fence.

use super::{Args, Fail, Lazy};
use crate::{AssessmentState, Store};
use serde_json::{Value, json};

const PROTOCOL: &str = "assessment/1";

use crate::assessment_publication as publication;

#[cfg(test)]
mod context_tests;

pub(super) fn call<F: FnOnce() -> Result<Store, String>>(
    args: &mut Args,
    board: &mut Lazy<F>,
) -> Result<Value, Fail> {
    let slug: String = args.need("slug")?;
    if args.method == "assessment_state" {
        args.end()?;
        let state = board.get()?.assessment_state(&slug)?;
        return Ok(json!({
            "protocol": PROTOCOL,
            "slug": state.slug,
            "status": state.status,
            "version": state.version,
        }));
    }
    let protocol: String = args.need("protocol")?;
    let record: String = args.need("record")?;
    let assessment: String = args.need("assessment")?;
    let actor = args.actor("actor")?;
    let expected: AssessmentState = args.need("expected")?;
    let publication = if protocol == "assessment/2" {
        Some(args.need::<publication::Publication>("publication")?)
    } else {
        None
    };
    args.end()?;
    if protocol != PROTOCOL && protocol != "assessment/2" {
        return Err(Fail::Error(format!(
            "record_waits: unsupported assessment protocol {protocol:?}"
        )));
    }
    let receipt = if let Some(publication) = &publication {
        let context = board.context.as_ref().ok_or_else(|| {
            Fail::Error(
                "assessment/2 requires explicit effective CLI context; evidence remains pending"
                    .into(),
            )
        })?;
        let evidence = publication::Evidence::admit(
            context,
            publication,
            &slug,
            &assessment,
            &record,
            &expected,
        )
        .map_err(Fail::Error)?;
        let mut current = false;
        let mut observe = || {
            let result = evidence.current().map_err(crate::Error::Db);
            current = result.as_ref().is_ok_and(|current| *current);
            result
        };
        let mut receipt = board.get()?.record_waits_guarded(
            &slug,
            &record,
            &assessment,
            &actor,
            &expected,
            &mut observe,
        )?;
        receipt.current &= current;
        receipt
    } else {
        board
            .get()?
            .record_waits_checked(&slug, &record, &assessment, &actor, &expected)?
    };
    let mut result = json!({
        "protocol": protocol,
        "slug": receipt.slug,
        "id": receipt.id,
        "record": receipt.record,
        "sid": receipt.sid,
        "at": receipt.at,
        "current": receipt.current,
    });
    if let Some(publication) = publication {
        result["publication"] = json!({
            "id": publication.id,
            "branch": publication.branch,
            "tip": publication.tip,
            "verification": publication.verification.as_deref(),
        });
    }
    Ok(result)
}
