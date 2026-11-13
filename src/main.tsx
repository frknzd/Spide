import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

window.addEventListener("contextmenu", (e) => e.preventDefault());
document.addEventListener("visibilitychange", () => document.documentElement.classList.toggle("paused", document.hidden));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
