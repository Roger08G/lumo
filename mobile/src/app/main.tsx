import ReactDOM from "react-dom/client";
import App from "@app/App.tsx";
import "@shared/index.css";
import { installViewportVariables } from "@shared/utils/viewport.ts";

installViewportVariables();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
