export class CassetteDB {
  constructor(path: string);
  insert(json: string): string;
  get(id: string): string | null;
  update(id: string, json: string): void;
  delete(id: string): void;
  query(query: string): string;
  dump(): string;
  compact(): void;
}
