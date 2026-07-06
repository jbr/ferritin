import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import prettier from "eslint-config-prettier";

// Modern flat-config equivalent of the setup across the other TSX apps:
// typescript-eslint recommended + react-hooks, Prettier owns formatting
// (eslint-config-prettier turns off conflicting rules), and explicit return
// types are not required. react-refresh is the Vite-specific addition.
export default tseslint.config(
  { ignores: ["dist", "src/api/schema.gen.ts"] },
  js.configs.recommended,
  tseslint.configs.recommended,
  // react-hooks 7's flat config declares `plugins` as an array, which ESLint 10
  // rejects; register the plugin as an object ourselves and borrow its rules.
  {
    plugins: { "react-hooks": reactHooks },
    rules: reactHooks.configs["recommended-latest"].rules,
  },
  reactRefresh.configs.vite,
  {
    rules: {
      "@typescript-eslint/explicit-function-return-type": "off",
    },
  },
  prettier,
);
