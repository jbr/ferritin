use crate::{
    request::Request,
    styled_string::{Document, DocumentNode, HeadingLevel, ListItem, ShowWhen, Span},
};

/// Structural model of the crate list, for `--format json`. `list` is a thin
/// command slated for rework, so this is a deliberately minimal JSON-only
/// projection — the terminal path ([`execute`]) is left untouched rather than
/// refactored into model+lower.
pub(crate) struct ListDoc {
    pub(crate) crates: Vec<CrateEntry>,
}

pub(crate) struct CrateEntry {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) is_default: bool,
    pub(crate) is_workspace: bool,
    pub(crate) used_by: Vec<String>,
    pub(crate) description: Option<String>,
}

/// Build the structural [`ListDoc`] for the JSON output (crates sorted by name,
/// matching [`execute`]).
pub(crate) fn json_model(request: &Request<'_>) -> ListDoc {
    let mut available = request
        .navigator()
        .list_available_crates()
        .collect::<Vec<_>>();
    available.sort_by(|a, b| a.name().cmp(b.name()));

    let crates = available
        .iter()
        .map(|c| CrateEntry {
            name: c.name().to_string(),
            version: c.version().to_string(),
            is_default: c.is_default_crate(),
            is_workspace: c.provenance().is_workspace(),
            used_by: c.used_by().iter().map(|u| u.to_string()).collect(),
            description: c.description().as_ref().map(|d| d.to_string()),
        })
        .collect();

    ListDoc { crates }
}

pub(crate) fn execute<'a>(request: &mut Request<'a>) -> (Document<'a>, bool, Option<&'a str>) {
    let mut nodes = vec![DocumentNode::Heading {
        level: HeadingLevel::Title,
        spans: vec![Span::plain("Available crates:")],
    }];

    let mut list_items = vec![];

    log::info!("Listing available crates");

    let mut available_crates = request
        .navigator()
        .list_available_crates()
        .collect::<Vec<_>>();

    log::info!(
        "Listing available crates ({} found)",
        available_crates.len()
    );

    available_crates.sort_by(|a, b| a.name().cmp(b.name()));

    // Find the default crate if any
    let default_crate = available_crates
        .iter()
        .find(|c| c.is_default_crate())
        .map(|c| c.name());

    // If no local project, show helpful message
    if request.navigator().local_source().is_none() {
        nodes.push(DocumentNode::paragraph(vec![Span::plain(
            "No Rust project detected. You can still navigate to:",
        )]));
    }

    // Format all crates uniformly - extract all needed data to avoid lifetime issues
    for crate_info in available_crates {
        let crate_name = crate_info.name().to_string();
        let is_default = crate_info.is_default_crate();
        let is_workspace = crate_info.provenance().is_workspace();
        let version = crate_info.version();
        let used_by = crate_info.used_by();
        let description = crate_info.description().as_ref().map(|d| d.to_string());

        let mut spans = vec![];
        if is_default {
            spans.push(Span::plain(" (workspace-local, aliased as "));
            spans.push(Span::strong("crate"));
            spans.push(Span::plain(")"));
        } else if is_workspace {
            spans.push(Span::plain(" (workspace-local)"));
        } else {
            spans.push(Span::plain(format!(" {version}")));

            if !used_by.is_empty() {
                spans.push(Span::plain(" ("));
                for (n, used_by) in used_by.iter().enumerate() {
                    if n != 0 {
                        spans.push(Span::plain(", "));
                    }
                    spans.push(Span::emphasis(used_by.to_string()));
                }
                spans.push(Span::plain(")"));
            }
        }

        if let Some(description) = description {
            let description = description.replace('\n', " ");
            spans.push(Span::plain("\n    "));
            spans.push(Span::plain(description));
        }

        // Prepend crate name label to spans
        let mut all_spans = vec![Span::strong(crate_name.clone()).with_path(crate_name)];
        if !spans.is_empty() {
            all_spans.push(Span::plain(" "));
            all_spans.extend(spans);
        }

        list_items.push(ListItem::new(vec![DocumentNode::paragraph(all_spans)]));
    }

    nodes.push(DocumentNode::List { items: list_items });

    // Show usage hints only in interactive mode when no local project
    if request.navigator().local_source().is_none() {
        nodes.push(DocumentNode::Conditional {
            show_when: ShowWhen::Interactive,
            nodes: vec![DocumentNode::paragraph(vec![Span::plain(
                "To navigate:\n• Press 'g' and enter a path like \"std::vec::Vec\"\n• Press 's' \
                 to search within a crate\n• Click on any item above to explore\n\nTo view \
                 documentation for a specific crate from docs.rs:\n• Press 'g' and enter \
                 \"crate_name\" or \"crate_name::Item\"",
            )])],
        });
    }

    (Document::from(nodes), false, default_crate)
}
