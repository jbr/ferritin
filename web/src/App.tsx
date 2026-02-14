import { Router, Route } from 'rhoto-router';
import { useEffect, useState } from 'react';
import { DocumentRenderer } from './components/DocumentRenderer';
import { fetchItem, listCrates } from './api/client';
import type { JsonDocument } from './types/api';

function ItemPage({ itemPath }: { itemPath: string }) {
  const [document, setDocument] = useState<JsonDocument | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    window.scrollTo(0, 0);
    fetchItem(itemPath)
      .then(doc => setDocument(doc))
      .catch(err => setError(err.message));
  }, [itemPath]);

  if (error) {
    return <div className="error">Error: {error}</div>;
  }

  if (!document) {
    return <div className="loading">Loading...</div>;
  }

  return <DocumentRenderer document={document} />;
}

function ListPage() {
  const [document, setDocument] = useState<JsonDocument | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listCrates()
      .then(doc => setDocument(doc))
      .catch(err => setError(err.message));
  }, []);

  if (error) {
    return <div className="error">Error: {error}</div>;
  }

  if (!document) {
    return <div className="loading">Loading...</div>;
  }

  return <DocumentRenderer document={document} />;
}

export function App() {
  return (
    <Router>
      <Route path="/" exact>
        <ListPage />
      </Route>
      <Route path="/*item">
        {(params) => <ItemPage itemPath={params.item} />}
      </Route>
    </Router>
  );
}
