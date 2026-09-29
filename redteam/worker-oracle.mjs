// Independent oracle: regexes copied verbatim from RISE worker/jev-recommend.mjs @082b3fa.
import fs from 'node:fs';
function requestsNoSound(intent) {
  const text = String(intent || '').normalize('NFKC').toLowerCase();
  return /\b(?:no|without|skip|avoid|mute|muted|zero)\s+(?:any\s+)?(?:audio|music|sounds?|soundscape|beat|soundtrack)\b|\b(?:silent|silence|muted)\b|\bsound\s+off\b/u.test(text);
}
function requestsNightDrive(intent) {
  const text = String(intent || '').normalize('NFKC').toLowerCase();
  return /\b(?:drift(?:s|ing)?|night[\s-]?driv(?:e|es|ing)|racing|race\s*cars?|street\s*rac\w*|highway|synthwave|outrun|tokyo|neon)\b/u.test(text);
}
function requestsNoVisualMotion(intent) {
  const text = intent.normalize('NFKC').toLowerCase();
  return /\b(?:no|without)\s+(?:moving\s+visuals?|visual\s+motion|motion|visuals?|animation)\b|\b(?:don['’]?t|do\s+not)\s+want\s+(?:any\s+)?moving\s+visuals?\b|\b(?:dark|black)\s+screen\b|\btext\s+only\b/u.test(text);
}
const explicitNoVisual = i => requestsNoVisualMotion(i)
  || /\b(?:no|without|skip|avoid|disable|turn off)\s+(?:any\s+)?visuals?\b|\bvisuals?\s+(?:off|disabled?)\b|\b(?:text|reading)\s+only\b/iu.test(i);
const lines = fs.readFileSync(process.argv[2], 'utf8').trim().split('\n');
let dis = 0;
for (const l of lines) {
  const [text, rules = '', narrow] = l.split('\t');
  const oSilent = rules.includes('LoudnessAtMost(0)'), wSilent = requestsNoSound(text);
  const oOff = rules.includes('VisualOff'), wOff = explicitNoVisual(text);
  const d = [];
  if (oSilent !== wSilent) d.push(`silent oracle=${oSilent} worker=${wSilent}`);
  if (oOff !== wOff) d.push(`visualsOff oracle=${oOff} worker=${wOff}`);
  if (process.argv[3] === 'nd' ) d.push(`nightDrive worker=${requestsNightDrive(text)} narrow=${narrow}`);
  if (d.length) { dis++; console.log(`${JSON.stringify(text)}  ${d.join('; ')}`); }
}
console.log(`${dis} of ${lines.length} texts disagree`);
