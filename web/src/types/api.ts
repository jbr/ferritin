export interface JsonDocument {
  canonicalUrl?: string;
  nodes: JsonNode[];
}

export type JsonNode =
  | { type: 'paragraph'; spans: JsonSpan[] }
  | { type: 'heading'; level: HeadingLevel; spans: JsonSpan[] }
  | { type: 'section'; title?: JsonSpan[]; nodes: JsonNode[] }
  | { type: 'list'; items: JsonListItem[] }
  | { type: 'codeBlock'; lang?: string; code: string }
  | { type: 'generatedCode'; spans: JsonSpan[] }
  | { type: 'horizontalRule' }
  | { type: 'blockQuote'; nodes: JsonNode[] }
  | { type: 'table'; header?: JsonTableCell[]; rows: JsonTableCell[][] };

export interface JsonSpan {
  text: string;
  style: SpanStyle;
  url?: string;
}

export interface JsonListItem {
  content: JsonNode[];
}

export interface JsonTableCell {
  spans: JsonSpan[];
}

export type HeadingLevel = 'Title' | 'Section';

export type SpanStyle =
  | 'Keyword'
  | 'TypeName'
  | 'FunctionName'
  | 'FieldName'
  | 'Lifetime'
  | 'Generic'
  | 'Plain'
  | 'Punctuation'
  | 'Operator'
  | 'Comment'
  | 'InlineRustCode'
  | 'InlineCode'
  | 'Strong'
  | 'Emphasis'
  | 'Strikethrough';
