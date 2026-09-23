// Formats an export off the main thread, in chunks, and hands back a Blob
// (PLAN.md § Exporting words: "in a worker and in chunks so a 300,000-question
// list doesn't block the page, and handed over as a Blob").
import { formatExport, type ExportInput } from './format';

self.onmessage = (ev: MessageEvent<{ input: ExportInput; type: string }>) => {
	const parts: string[] = [];
	for (const chunk of formatExport(ev.data.input)) parts.push(chunk);
	const blob = new Blob(parts, { type: ev.data.type });
	(self as unknown as Worker).postMessage(blob);
};
