//! Wega-only protocol mapping over the authoritative style inspection contract.
use std::time::Instant;
use wega_dom::{
    NodeKey,
    inspect::{self as contract, StyleInspect},
};
use wega_protocol::inspector::{
    InspectorError, InspectorErrorCode, dom::InspectorNodeId, styles::*,
};
fn error(code: InspectorErrorCode, message: &str) -> InspectorError {
    InspectorError::new(code, message)
}
fn convert(e: contract::StyleReadError) -> InspectorError {
    use contract::StyleReadError::*;
    error(
        match e {
            Stale => InspectorErrorCode::StaleNode,
            Invalid => InspectorErrorCode::InvalidPayload,
            Depth => InspectorErrorCode::BudgetExceeded,
            Timeout => InspectorErrorCode::Timeout,
        },
        "Style read refused by document contract",
    )
}
pub fn inspect<D: StyleInspect>(
    doc: &D,
    mut scope: StyleScope,
    query: &StyleQuery,
) -> Result<StylePage, InspectorError> {
    let start = Instant::now();
    scope.epoch = doc.mutation_epoch().as_u64();
    scope.generation = doc.style_generation();
    if !query.valid() {
        return Err(error(
            InspectorErrorCode::InvalidPayload,
            "Invalid style query",
        ));
    }
    if query.node().is_some_and(|n| n.document != scope.document) {
        return Err(error(
            InspectorErrorCode::StaleDocument,
            "Style node belongs to a replaced document",
        ));
    }
    if let Some(r) = query.reference() {
        if r.scope.target != scope.target || r.scope.session != scope.session {
            return Err(error(
                InspectorErrorCode::Unauthorized,
                "Rule belongs to another inspector target/session",
            ));
        }
        if r.scope.document != scope.document {
            return Err(error(
                InspectorErrorCode::StaleDocument,
                "Rule document was replaced",
            ));
        }
        if r.scope.epoch != scope.epoch || r.scope.generation != scope.generation {
            return Err(error(
                InspectorErrorCode::StaleMutation,
                "Rule reference was invalidated",
            ));
        }
    }
    let key = |n: InspectorNodeId| NodeKey::from_u64(n.key);
    let read = match query {
        StyleQuery::Sheets { .. } => contract::StyleRead::Sheets,
        StyleQuery::Rules { parent, .. } => contract::StyleRead::Rules {
            owner: key(parent.owner),
            path: parent.path.clone(),
        },
        StyleQuery::Declarations { rule, .. } => contract::StyleRead::Declarations {
            owner: key(rule.owner),
            path: rule.path.clone(),
        },
        StyleQuery::Inline { node, .. } => contract::StyleRead::Inline(key(*node)),
        StyleQuery::Computed { node, .. } => contract::StyleRead::Computed(key(*node)),
        StyleQuery::Matched { node } => contract::StyleRead::Matched(key(*node)),
    };
    let data = doc.read_style(&read, query.offset()).map_err(convert)?;
    let availability = match data.availability {
        contract::Availability::Available => StyleAvailability::Available,
        contract::Availability::Cached => StyleAvailability::Cached,
        contract::Availability::NotComputed => StyleAvailability::NotComputed,
        contract::Availability::Unsupported => StyleAvailability::Unsupported,
    };
    let note=match query{
  StyleQuery::Computed{..}=>"Cached scalar longhands only (23). Other longhands, custom/pseudo/visited styles and used-value resolution unsupported. No inspector layout flush.",
  StyleQuery::Matched{..}=>"Unsupported: retained declaration blocks do not identify matched selectors/rules, specificity or cascade winners.",
  StyleQuery::Sheets{..}=>"Author sheets only, owner-key order. UA/user/imported child sheets and cascade order unsupported.",
  StyleQuery::Rules{..}=>"Selectors are rule selector lists, not matched selectors. Locations are stylesheet-relative start line/column. Specificity, end ranges and cascade ranking unsupported.",
  _=>"Canonical declaration longhands; shorthand/source formatting and cascade winning status unsupported.",
 }.into();
    let mut page = StylePage {
        scope,
        query: query.clone(),
        availability,
        layout_pending: data.layout_pending,
        total: data.total,
        next: data.next,
        items: vec![],
        truncated: data.truncated,
        note,
    };
    let reference = |owner: NodeKey, path: Vec<u32>| RuleRef {
        scope,
        owner: InspectorNodeId {
            document: scope.document,
            key: owner.as_u64(),
        },
        path,
    };
    let mut bytes = 2048;
    for item in data.items {
        let value = match item {
            contract::StyleItem::Sheet(s) => StyleItem::Sheet {
                reference: reference(s.owner, vec![]),
                url: s.url,
                disabled: s.disabled,
                rules: s.rules,
                truncated: s.truncated,
            },
            contract::StyleItem::Rule(r) => StyleItem::Rule {
                reference: reference(
                    key(query
                        .reference()
                        .ok_or_else(|| {
                            error(InspectorErrorCode::InvalidPayload, "Rule list needs owner")
                        })?
                        .owner),
                    r.path,
                ),
                kind: r.kind,
                selector: r.selector,
                children: r.children,
                declarations: r.declarations,
                source: r.source.map(|s| StyleSource {
                    url: s.url,
                    line: s.line,
                    column: s.column,
                }),
                truncated: r.truncated,
            },
            contract::StyleItem::Declaration(d) => StyleItem::Declaration {
                name: d.name,
                value: d.value,
                important: if matches!(query, StyleQuery::Computed { .. }) {
                    None
                } else {
                    Some(d.important)
                },
                truncated: d.truncated,
            },
        };
        let size = serde_json::to_vec(&value)
            .map_err(|_| {
                error(
                    InspectorErrorCode::InvalidPayload,
                    "Style serialization failed",
                )
            })?
            .len()
            + 1;
        page.truncated |= match &value {
            StyleItem::Sheet { truncated, .. }
            | StyleItem::Rule { truncated, .. }
            | StyleItem::Declaration { truncated, .. } => *truncated,
        };
        if bytes + size > STYLE_BYTES {
            page.next = Some(query.offset() + page.items.len() as u32);
            page.truncated = true;
            break;
        }
        bytes += size;
        page.items.push(value);
    }
    if !page.is_bounded() {
        return Err(error(
            InspectorErrorCode::BudgetExceeded,
            "Style response exceeds budget",
        ));
    }
    contract::BoundedCss::check(start).map_err(convert)?;
    Ok(page)
}
