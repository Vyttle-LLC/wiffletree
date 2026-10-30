// markdownlint-cli2 output formatter for the brain gate (gate/checks/markdown_lint.py).
// It writes each finding's file, line, column, severity and rule as JSON to
// $BRAIN_GATE_MARKDOWNLINT_REPORT. Descriptions and details, which can quote
// the file's text, are left out.
const fs = require("node:fs");

module.exports = async ({ results }) => {
  const findings = results.map((result) => ({
    file: result.fileName,
    line: result.lineNumber,
    column: result.errorRange ? result.errorRange[0] : null,
    severity: result.severity,
    rule: result.ruleNames.join("/"),
  }));
  fs.writeFileSync(process.env.BRAIN_GATE_MARKDOWNLINT_REPORT, JSON.stringify(findings));
};
