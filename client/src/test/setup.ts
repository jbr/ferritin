// Extends `expect` with jest-dom matchers (toBeInTheDocument, etc.) and unmounts
// React trees between tests. We don't use Vitest globals, so cleanup is wired
// explicitly rather than via an auto-registered global `afterEach`.
import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

afterEach(cleanup);
