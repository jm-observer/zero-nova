import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(__dirname, '..');
const templatePath = path.join(__dirname, 'generated-types.template.ts');
const outputPath = path.join(projectRoot, 'src/generated/generated-types.ts');

const content = await fs.readFile(templatePath, 'utf8');
await fs.mkdir(path.dirname(outputPath), { recursive: true });
await fs.writeFile(outputPath, content, 'utf8');
