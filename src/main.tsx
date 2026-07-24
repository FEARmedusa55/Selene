import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

/* Load order matters: the token contract first, then themes (which override
   tokens under their [data-theme] selector), then component styles that
   consume them. */
import "./styles/tokens.css";
import "./styles/themes/lunar.css";
import "./styles/themes/steam-dark.css";
import "./styles/themes/daylight.css";
import "./styles/base.css";
import "./styles/app.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
