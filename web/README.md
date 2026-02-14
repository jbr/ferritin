# Ferritin Web Frontend

React-based web interface for browsing Rust documentation.

## Development

```bash
# Install dependencies
npm install

# Start dev server (proxies /api to localhost:8080)
npm run dev

# Build for production
npm run build
```

## Architecture

- **API Client** (`src/api/client.ts`) - Fetches JSON documents from the backend API
- **TypeScript Types** (`src/types/api.ts`) - Matches the JSON document format from the server
- **Components**:
  - `DocumentRenderer` - Top-level document rendering
  - `NodeRenderer` - Renders individual document nodes (paragraphs, headings, lists, etc.)
  - `SpanRenderer` - Renders styled text spans with optional links
- **Routing** - React Router handles navigation between crates/items

## Running

1. Start the backend server: `cargo run --features serve-json -- serve`
2. Start the frontend dev server: `npm run dev`
3. Open http://localhost:5173 in your browser
