use crate::format_context::FormatContext;
use crate::indent::Indent;
use crate::request::Request;
use crate::state::RustdocTools;
use crate::traits::WriteFmt;
use anyhow::Result;
use ferritin_common::search::indexer::{BM25Scorer, SearchIndex};
use mcplease::traits::{Tool, WithExamples};
use mcplease::types::Example;
use serde::{Deserialize, Serialize};

/// Search for items within a specific crate
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, clap::Args)]
#[serde(rename = "search")]
pub struct Search {
    /// The crate to search within. Use `crate` for the current crate.
    pub crate_name: String,

    /// The search query to look for. Individual terms will be combined additively.
    pub query: String,

    /// Maximum number of results to return (default: 10)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(short, long)]
    pub limit: Option<usize>,
}

impl WithExamples for Search {
    fn examples() -> Vec<Example<Self>> {
        vec![
            Example {
                description: "Search for 'Error' in std crate",
                item: Self {
                    crate_name: "std".into(),
                    query: "Error".into(),
                    limit: Some(5),
                },
            },
            Example {
                description: "Search for 'iterator items' in current crate",
                item: Self {
                    crate_name: "crate".into(),
                    query: "iterator items".into(),
                    limit: None,
                },
            },
        ]
    }
}

impl Tool<RustdocTools> for Search {
    fn execute(self, state: &mut RustdocTools) -> Result<String> {
        let manifest_path = state.resolve_path("Cargo.toml", None)?;

        let request = Request::new(manifest_path);
        let index = match SearchIndex::load_or_build(&request, &self.crate_name) {
            Ok(index) => index,
            Err(mut suggestions) => {
                let mut result = format!(
                    "`{}` not found. Did you mean one of these?\n\n",
                    &self.crate_name
                );
                suggestions.sort_by(|a, b| b.score().total_cmp(&a.score()));
                for suggestion in suggestions.into_iter().take(5).filter(|s| s.score() > 0.8) {
                    result.write_fmt(format_args!("• `{}` ", suggestion.path()));

                    if let Some(item) = suggestion.item() {
                        result.write_fmt(format_args!("({:?})\n", item.kind()));
                    } else {
                        result.push_str("(Crate)\n");
                    }
                }
                return Ok(result);
            }
        };

        // Perform search with BM25 scoring
        let limit = self.limit.unwrap_or(10);
        let search_results = index.search(&self.query);
        let mut scorer = BM25Scorer::new();
        scorer.add(&self.crate_name, search_results);
        let results = scorer.score();

        // Format results
        let mut output = String::new();
        output.write_fmt(format_args!(
            "Search results for '{}' in crate '{}':\n\n",
            self.query,
            index.crate_name()
        ));

        if results.is_empty() {
            output.push_str("No results found.\n");
        } else {
            let total_score: f32 = results.iter().map(|r| r.score).sum();
            let mut cumulative_score = 0.0;
            let min_results = 1;

            let top_score = results.first().map(|r| r.score).unwrap_or(0.0);
            let mut prev_score = top_score;

            for (i, result) in results.into_iter().take(limit).enumerate() {
                if i >= min_results
                    && (result.score / top_score < 0.05
                        || result.score / prev_score < 0.5
                        || cumulative_score / total_score > 0.3)
                {
                    break;
                }

                if let Some((item, path)) =
                    request.get_item_from_id_path(&self.crate_name, &result.id_path)
                {
                    cumulative_score += result.score;
                    prev_score = result.score;
                    let path = path.join("::");
                    let normalized_score = 100.0 * result.score / total_score;
                    output.write_fmt(format_args!(
                        "• {path} ({:?}) - score: {normalized_score:.0}\n",
                        item.kind()
                    ));

                    if let Some(docs) = request.docs_to_show(item, true, &FormatContext::default())
                    {
                        output.write_fmt(format_args!("{}", Indent::new(&docs, 4)));
                    }
                }
            }
        }

        Ok(output)
    }
}
