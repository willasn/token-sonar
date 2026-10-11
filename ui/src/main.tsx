/* TokenSonar · 前端入口 */
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { StoreProvider } from "./state/store";
import { UiProvider } from "./components/ui";

import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/components.css";
import "./styles/views.css";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <UiProvider>
        <StoreProvider>
          <App />
        </StoreProvider>
      </UiProvider>
    </StrictMode>
  );
}
