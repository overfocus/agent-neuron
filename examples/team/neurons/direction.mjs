// The direction's stdout summary (SPEC 3.1): stdout is the answer, one JSON
// value, and with no outgoing links the run's stdout prints it - the demo
// prints the same answer live from the run's --stream. The tally's direction
// value in, the flat sentence out.
let raw = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (c) => (raw += c));
process.stdin.on('end', () => {
  const v = JSON.parse(raw);
  const line = v?.mode === 'quorum' ? `settled by quorum: ${v.branch}` : 'tie — the lead decides';
  process.stdout.write(JSON.stringify(line));
});
