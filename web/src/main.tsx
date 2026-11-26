import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";

const surface = window.location.hash.replace("#", "") || "dock";

function Proto() {
  return <div className="proto">Solyra · {surface}</div>;
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Proto />);
