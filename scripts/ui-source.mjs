// Source JavaScript de l'interface, dans l'ordre de chargement de ui/index.html, pour les tests sans navigateur.
import {readFileSync} from "node:fs";

const root = new URL("../ui/", import.meta.url);
export const page = readFileSync(new URL("index.html", root), "utf8");
export const scripts = [...page.matchAll(/<script src="\/ui\/([^"]+)"><\/script>/g)].map(m => m[1]);
export const source = scripts.map(name => readFileSync(new URL(name, root), "utf8")).join("\n");
