// Level 0: reads one drafted note on stdin and proposes a tighter wording. A stand-in for a model call: it only drops filler.
let input = '';
process.stdin.on('data', (c) => (input += c));
process.stdin.on('end', () => {
  const draft = String(JSON.parse(input).text);
  const proposal = draft
    .replace(/\b(It is important to note that|Please note that|In order to)\s+/gi, (m) => (/in order to/i.test(m) ? 'To ' : ''))
    .replace(/\b(very|really|basically|actually)\s+/gi, '')
    .replace(/^\w/, (c) => c.toUpperCase());
  process.stdout.write(JSON.stringify({ draft, proposal }));
});
