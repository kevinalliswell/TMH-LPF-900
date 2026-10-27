/** Return checked milestone line numbers without a non-empty evidence reference. */
export function missingMilestoneEvidence(markdown) {
  return markdown.split(/\r?\n/).flatMap((line, index) => {
    if (!/^\s*-\s*\[[xX]\]/.test(line)) return [];
    return /（验收证据：[^）]*\S[^）]*）/.test(line) ? [] : [index + 1];
  });
}
