use crate::request::Request;
use crate::styled_string::{Document, DocumentNode, HeadingLevel, ListItem, ShowWhen, Span};

pub(crate) fn execute<'a>(request: &'a Request) -> (Document<'a>, bool) {
    let mut nodes = vec![
        DocumentNode::Heading {
            level: HeadingLevel::Title,
            spans: vec![Span::plain("Available crates:")],
        },
        DocumentNode::Span(Span::plain("\n")),
    ];

    let mut list_items = vec![];

    let mut available_crates = request.list_available_crates().collect::<Vec<_>>();

    available_crates.sort_by(|a, b| a.name().cmp(b.name()));

    // If no local project, show helpful message
    if request.local_source().is_none() {
        nodes.insert(
            1,
            DocumentNode::Span(Span::plain(
                "No Rust project detected. You can still navigate to:\n\n",
            )),
        );
    }

    // Format all crates uniformly - extract all needed data to avoid lifetime issues
    for crate_info in available_crates {
        let crate_name = crate_info.name().to_string();
        let is_default = crate_info.is_default_crate();
        let is_workspace = crate_info.provenance().is_workspace();
        let version = crate_info.version();
        let used_by = crate_info.used_by();
        let description = crate_info.description().as_ref().map(|d| d.to_string());
        let mut item_nodes = vec![];
        if is_default {
            item_nodes.push(DocumentNode::Span(Span::plain(
                " (workspace-local, aliased as ",
            )));
            item_nodes.push(DocumentNode::Span(Span::strong("crate")));
            item_nodes.push(DocumentNode::Span(Span::plain(")")));
        } else if is_workspace {
            item_nodes.push(DocumentNode::Span(Span::plain(" (workspace-local)")));
        } else {
            if let Some(version) = version {
                item_nodes.push(DocumentNode::Span(Span::plain(format!(" {version}"))));
            }

            if !used_by.is_empty() {
                item_nodes.push(DocumentNode::Span(Span::plain(" (")));
                for (n, used_by) in used_by.iter().enumerate() {
                    if n != 0 {
                        item_nodes.push(DocumentNode::Span(Span::plain(", ")));
                    }
                    item_nodes.push(DocumentNode::Span(Span::emphasis(used_by.to_string())));
                }
                item_nodes.push(DocumentNode::Span(Span::plain(")")));
            }
        }

        if let Some(description) = description {
            let description = description.replace('\n', " ");
            item_nodes.push(DocumentNode::Span(Span::plain("\n    ")));
            item_nodes.push(DocumentNode::Span(Span::plain(description)));
        }

        list_items.push(ListItem::labeled(
            vec![Span::strong(crate_name.clone()).with_path(crate_name)],
            item_nodes,
        ));
    }

    nodes.push(DocumentNode::List { items: list_items });

    // Show usage hints only in interactive mode when no local project
    if request.local_source().is_none() {
        nodes.push(DocumentNode::Conditional {
            show_when: ShowWhen::Interactive,
            nodes: vec![DocumentNode::Span(Span::plain(
                "\n\
                To navigate:\n\
                • Press 'g' and enter a path like \"std::vec::Vec\"\n\
                • Press 's' to search within a crate\n\
                • Click on any item above to explore\n\n\
                To view documentation for a specific crate from docs.rs:\n\
                • Press 'g' and enter \"crate_name\" or \"crate_name::Item\"\n",
            ))],
        });
    }

    (Document::from(nodes), false)
}
