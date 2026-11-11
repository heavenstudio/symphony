//! Explicit assessment capability and the checked occurrence writer. Unknown arguments and
//! incompatible protocols fail before connecting; the store owns every authorization/fence.

use super::{Args, Fail, Lazy, Py};
use crate::{AssessmentState, Store};

const PROTOCOL: &str = "assessment/1";

use crate::assessment_publication as publication;

#[cfg(test)]
mod context_tests;

pub(super) fn call<F: FnOnce() -> Result<Store, String>>(
    args: &mut Args,
    board: &mut Lazy<F>,
) -> Result<Py, Fail> {
    let slug: String = args.need("slug")?;
    if args.method == "assessment_state" {
        args.end()?;
        let state = board.get()?.assessment_state(&slug)?;
        return Ok(Py::obj(vec![
            ("protocol", Py::str(PROTOCOL)),
            ("slug", Py::Str(state.slug)),
            ("status", Py::Str(state.status)),
            ("version", Py::Int(state.version)),
        ]));
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
    let mut result = vec![
        ("protocol", Py::str(&protocol)),
        ("slug", Py::Str(receipt.slug)),
        ("id", Py::Str(receipt.id)),
        ("record", Py::Str(receipt.record)),
        ("sid", Py::Int(receipt.sid)),
        ("at", Py::Float(receipt.at)),
        ("current", Py::Bool(receipt.current)),
    ];
    if let Some(publication) = publication {
        result.push((
            "publication",
            Py::obj(vec![
                ("id", Py::Str(publication.id)),
                ("branch", Py::Str(publication.branch)),
                ("tip", Py::Str(publication.tip)),
                (
                    "verification",
                    Py::opt_str(publication.verification.as_deref()),
                ),
            ]),
        ));
    }
    Ok(Py::obj(result))
}
