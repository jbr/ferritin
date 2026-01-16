use crate::request::Request;
use crate::styled_string::{Document, DocumentNode, HeadingLevel, ListItem, Span};

pub(crate) fn execute<'a>(request: &'a Request) -> (Document<'a>, bool) {
    let mut nodes = vec![
        DocumentNode::Heading {
            level: HeadingLevel::Title,
            spans: vec![Span::plain("Available crates:")],
        },
        DocumentNode::Span(Span::plain("\n")),
    ];

    let mut list_items = vec![];

    for crate_info in request.project.crate_info(None) {
        let crate_name = crate_info.name();

        let note = if crate_info.is_default_crate() {
            " (workspace-local, aliased as \"crate\")".to_string()
        } else if crate_info.crate_type().is_workspace() {
            " (workspace-local)".to_string()
        } else if let Some(version) = crate_info.version() {
            let dev_dep_note = if crate_info.is_dev_dep() {
                " (dev-dep)"
            } else {
                ""
            };

            // Show which workspace members use this dependency
            let usage_info = if !crate_info.used_by().is_empty() {
                let members: Vec<String> = crate_info
                    .used_by()
                    .iter()
                    .map(|member| {
                        if crate_info.is_dev_dep() {
                            format!("{} dev", member)
                        } else {
                            member.clone()
                        }
                    })
                    .collect();
                format!(" ({})", members.join(", "))
            } else {
                String::new()
            };

            format!(" {}{}{}", version, dev_dep_note, usage_info)
        } else {
            String::new()
        };

        let mut item_nodes = vec![DocumentNode::Span(Span::plain(note))];

        if let Some(description) = crate_info.description() {
            let description = description.replace('\n', " ");
            item_nodes.push(DocumentNode::Span(Span::plain("\n    ")));
            item_nodes.push(DocumentNode::Span(Span::plain(description)));
        }

        list_items.push(ListItem::labeled(vec![Span::plain(crate_name)], item_nodes));
    }

    nodes.push(DocumentNode::List { items: list_items });

    (Document::from(nodes), false)
}
