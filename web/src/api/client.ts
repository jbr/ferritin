import type { JsonDocument } from "../types/api";

export async function fetchItem(itemPath: string): Promise<JsonDocument> {
  // Accept both / and :: as path separators; normalize to ::
  const normalized = itemPath.replace(/\//g, "::");
  const response = await fetch(`/api/crates/${normalized}`);

  if (!response.ok) {
    throw new Error(`Failed to fetch: ${response.statusText}`);
  }

  return response.json();
}

export async function search(
  crateSpec: string,
  query: string
): Promise<JsonDocument> {
  const url = `/api/search/${crateSpec}?q=${encodeURIComponent(query)}`;
  const response = await fetch(url);

  if (!response.ok) {
    throw new Error(`Search failed: ${response.statusText}`);
  }

  return response.json();
}

export async function listCrates(): Promise<JsonDocument> {
  const response = await fetch("/api/crates");

  if (!response.ok) {
    throw new Error(`Failed to list crates: ${response.statusText}`);
  }

  return response.json();
}
