import React from "react";
import ReactDOM from "react-dom/client";
// @ts-ignore;
import UserComponent from "./UserComponent";
import "./globals.css";

const rootEl = document.getElementById("root");
if (rootEl) {
  ReactDOM.createRoot(rootEl).render(<UserComponent />);
}
