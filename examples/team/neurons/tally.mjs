// The quorum join (SPEC 5.2, on the file-mode pattern of timbre's reviewers:
// one threshold-2 input from each reviewer on the one decision; timbre
// net/neurons/verdicts.mjs). Both IC answers arrive as text payloads (3.4: a
// person's answer is text), shaped by the pageserver as
//   "<branch> | why: <their one line>"  or simply "<branch>".
// It reads the wrapped input of a threshold above 1 { payload, inputs } and
// answers one direction value: quorum on agreement, a tie on disagreement.
// stdout is the answer and must be one JSON value; the gate item it returns
// is what the lead's page shows.
let raw = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (c) => (raw += c));
process.stdin.on('end', () => {
  const wrapped = JSON.parse(raw);
  const inputs = Array.isArray(wrapped?.inputs) ? wrapped.inputs : [];
  if (inputs.length < 2) {
    process.stderr.write('the join fired without a review from each IC\n');
    process.exit(1);
  }
  const trim = (s) => String(s).trim();
  const vote = (text) => trim(text).split('|')[0].trim().split(/\s+/)[0];
  const why = (text) => {
    const at = trim(text).indexOf('|');
    if (at === -1) return '';
    return trim(text)
      .slice(at + 1)
      .trim()
      .replace(/^why:\s*/i, '')
      .trim();
  };
  const votes = inputs.map((t) => vote(trim(t)));
  const whys = inputs.map((t) => why(t));
  const [v1, v2] = votes;
  const quorum = v1 && v1 === v2;
  const seen = [...new Set(votes.filter(Boolean))];
  const options = seen.map((branch) => ({
    id: `merge-${branch.replace(/[^a-z0-9]+/gi, '-')}`,
    label: `Merge ${branch}`,
    branch,
  }));
  const gate = quorum
    ? {
        id: 'team-demo-gate-1',
        answerable_by: 'lead',
        mode: 'quorum',
        branch: v1,
        title: `The ICs settled on ${v1}`,
        why: `quorum: both IC votes name ${v1}.${whys[0] ? ` first why: ${whys[0]}.` : ''}${whys[1] ? ` second why: ${whys[1]}.` : ''}`,
        draft: 'main as it stands: the pier module, untouched by either patch',
        proposal: `merge ${v1}`,
        notes: [
          'Sam gates the merge: Merge the branch, or Refuse.',
          'A refusal merges nothing and is recorded in the ledger.',
        ],
        options,
      }
    : {
        id: 'team-demo-gate-1',
        answerable_by: 'lead',
        mode: 'tie',
        title: `The vote is a tie: ${seen.join(' vs ')} - the lead decides`,
        why: `the two IC votes came in as ${votes.map((v) => v || '(unparseable)').join(' and ')}; the lead picks one patch or refuses.`,
        draft: 'main as it stands: the pier module, untouched by either patch',
        proposal: "the lead's call: Merge one of the options below, or Refuse",
        notes: [
          'Sam breaks the tie: Merge one patch, or Refuse both.',
          'A refusal merges nothing and is recorded in the ledger.',
        ],
        options,
      };
  process.stdout.write(JSON.stringify(gate));
});
