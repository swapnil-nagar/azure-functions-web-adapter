// Wrapper to launch the Next.js standalone server on the port
// expected by the Azure Functions Web Adapter.
const path = require("path");

process.env.PORT = process.env.PORT || "8080";
process.env.HOSTNAME = "0.0.0.0";

require(path.join(__dirname, ".next", "standalone", "server.js"));
