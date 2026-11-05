// Level 0, an effect: "publishes" the approved text. Here it only answers with it; a real one would write it somewhere.
let input = '';
process.stdin.on('data', (c) => (input += c));
process.stdin.on('end', () => process.stdout.write(JSON.stringify({ published: JSON.parse(input) })));
