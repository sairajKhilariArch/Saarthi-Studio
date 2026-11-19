// The desktop's bounded JSON Schema check for MCP tool arguments
// (app/src/mcp.rs: validate_arguments), ported so the VS Code built-in server
// refuses the same malformed call the desktop would.
//
// Keyword subset: type, enum, const, required, properties,
// additionalProperties, items (schema and tuple form), minItems/maxItems,
// uniqueItems, minLength/maxLength, minimum/maximum/exclusiveMinimum/
// exclusiveMaximum, multipleOf, minProperties/maxProperties, allOf/anyOf/
// oneOf/not and local `$ref`s. `pattern`, `format`, `propertyNames` and remote
// references are left to the server, because it is the authority on its own
// tool: refusing a call a server would have accepted is worse than letting the
// server reject one with its own message.

const DEPTH_LIMIT = 16;

function isObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

/** JSON Schema counts 1.0 as an integer; a model sending a float meant the whole number. */
function matchesType(expected: string, value: unknown): boolean {
  switch (expected) {
    case "string": return typeof value === "string";
    case "number": return typeof value === "number" && Number.isFinite(value);
    case "integer": return typeof value === "number" && Number.isInteger(value);
    case "boolean": return typeof value === "boolean";
    case "object": return isObject(value);
    case "array": return Array.isArray(value);
    case "null": return value === null;
    default: return true;
  }
}

function argumentLabel(path: string): string {
  return path ? `Argument "${path}"` : "The arguments";
}

function childPath(path: string, name: string): string {
  return path ? `${path}.${name}` : name;
}

function indexPath(path: string, index: number): string {
  return `${path}[${index}]`;
}

function deepEqual(left: unknown, right: unknown): boolean {
  if (left === right) return true;
  if (Array.isArray(left) && Array.isArray(right)) {
    return left.length === right.length && left.every((entry, index) => deepEqual(entry, right[index]));
  }
  if (isObject(left) && isObject(right)) {
    const leftKeys = Object.keys(left);
    const rightKeys = Object.keys(right);
    return leftKeys.length === rightKeys.length && leftKeys.every((key) => deepEqual(left[key], right[key]));
  }
  return false;
}

/** Only document-local references are followed; a remote `$ref` is the server's. */
function resolveRef(root: unknown, reference: string): unknown {
  if (!reference.startsWith("#")) return undefined;
  const pointer = reference.slice(1);
  if (!pointer.startsWith("/")) return undefined;
  let current: unknown = root;
  for (const raw of pointer.slice(1).split("/")) {
    const key = raw.replace(/~1/g, "/").replace(/~0/g, "~");
    if (Array.isArray(current)) {
      const index = Number(key);
      if (!Number.isInteger(index) || index < 0 || index >= current.length) return undefined;
      current = current[index];
    } else if (isObject(current) && Object.prototype.hasOwnProperty.call(current, key)) {
      current = current[key];
    } else {
      return undefined;
    }
  }
  return current;
}

function limitOf(object: Record<string, unknown>, key: string): number | null {
  const value = object[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/** Null when the value is acceptable, else the reason a caller can read. */
function validateValue(
  root: unknown,
  schema: unknown,
  value: unknown,
  path: string,
  depth: number,
): string | null {
  if (depth > DEPTH_LIMIT) return null;
  // A boolean schema carries its whole meaning.
  if (typeof schema === "boolean") {
    if (schema) return null;
    return `${argumentLabel(path)} ${path ? "is" : "are"} not accepted by this tool.`;
  }
  // A schema this client cannot model must not refuse a valid call.
  if (!isObject(schema)) return null;

  if (typeof schema.$ref === "string") {
    // Sibling keywords next to `$ref` are ignored, as draft-4 reads it.
    const target = resolveRef(root, schema.$ref);
    if (target !== undefined) return validateValue(root, target, value, path, depth + 1);
  }

  const declared = schema.type;
  const expected = Array.isArray(declared)
    ? declared.filter((name): name is string => typeof name === "string")
    : typeof declared === "string" ? [declared] : [];
  if (expected.length && !expected.some((name) => matchesType(name, value))) {
    return `${argumentLabel(path)} must be ${expected.join(" or ")}.`;
  }

  if (Array.isArray(schema.enum) && !schema.enum.some((option) => deepEqual(option, value))) {
    const listed = schema.enum.slice(0, 6).map((option) => JSON.stringify(option)).join(", ");
    return `${argumentLabel(path)} must be one of ${listed}.`;
  }
  if (Object.prototype.hasOwnProperty.call(schema, "const") && !deepEqual(schema.const, value)) {
    return `${argumentLabel(path)} must be ${JSON.stringify(schema.const)}.`;
  }

  if (Array.isArray(schema.allOf)) {
    for (const branch of schema.allOf) {
      const problem = validateValue(root, branch, value, path, depth + 1);
      if (problem) return problem;
    }
  }
  if (Array.isArray(schema.anyOf) && schema.anyOf.length) {
    if (!schema.anyOf.some((branch) => validateValue(root, branch, value, path, depth + 1) === null)) {
      return `${argumentLabel(path)} must match one of ${schema.anyOf.length} declared forms.`;
    }
  }
  if (Array.isArray(schema.oneOf) && schema.oneOf.length) {
    const matched = schema.oneOf.filter(
      (branch) => validateValue(root, branch, value, path, depth + 1) === null,
    ).length;
    if (matched !== 1) {
      return `${argumentLabel(path)} must match exactly one of ${schema.oneOf.length} declared forms (${matched} matched).`;
    }
  }
  if (schema.not !== undefined && validateValue(root, schema.not, value, path, depth + 1) === null) {
    return `${argumentLabel(path)} uses a form this tool excludes.`;
  }

  if (typeof value === "string") {
    const length = [...value].length;
    const minLength = limitOf(schema, "minLength");
    if (minLength !== null && length < minLength) {
      return `${argumentLabel(path)} must be at least ${minLength} characters.`;
    }
    const maxLength = limitOf(schema, "maxLength");
    if (maxLength !== null && length > maxLength) {
      return `${argumentLabel(path)} must be at most ${maxLength} characters.`;
    }
  }

  if (typeof value === "number" && Number.isFinite(value)) {
    const minimum = limitOf(schema, "minimum");
    if (minimum !== null && value < minimum) return `${argumentLabel(path)} must be at least ${minimum}.`;
    const maximum = limitOf(schema, "maximum");
    if (maximum !== null && value > maximum) return `${argumentLabel(path)} must be at most ${maximum}.`;
    const exclusiveMinimum = limitOf(schema, "exclusiveMinimum");
    if (exclusiveMinimum !== null && value <= exclusiveMinimum) {
      return `${argumentLabel(path)} must be greater than ${exclusiveMinimum}.`;
    }
    const exclusiveMaximum = limitOf(schema, "exclusiveMaximum");
    if (exclusiveMaximum !== null && value >= exclusiveMaximum) {
      return `${argumentLabel(path)} must be less than ${exclusiveMaximum}.`;
    }
    const step = limitOf(schema, "multipleOf");
    if (step !== null && step > 0) {
      const quotient = value / step;
      if (Math.abs(quotient - Math.round(quotient)) > 1e-9) {
        return `${argumentLabel(path)} must be a multiple of ${step}.`;
      }
    }
  }

  if (Array.isArray(value)) {
    const minItems = limitOf(schema, "minItems");
    if (minItems !== null && value.length < minItems) {
      return `${argumentLabel(path)} must have at least ${minItems} items.`;
    }
    const maxItems = limitOf(schema, "maxItems");
    if (maxItems !== null && value.length > maxItems) {
      return `${argumentLabel(path)} must have at most ${maxItems} items.`;
    }
    if (schema.uniqueItems === true) {
      for (let index = 0; index < value.length; index += 1) {
        if (value.slice(0, index).some((entry) => deepEqual(entry, value[index]))) {
          return `${argumentLabel(path)} must not repeat the same item.`;
        }
      }
    }
    if (Array.isArray(schema.items)) {
      // Draft-4 tuple form: one schema per position.
      for (let index = 0; index < value.length; index += 1) {
        const branch = schema.items[index];
        if (branch === undefined) continue;
        const problem = validateValue(root, branch, value[index], indexPath(path, index), depth + 1);
        if (problem) return problem;
      }
    } else if (schema.items !== undefined) {
      for (let index = 0; index < value.length; index += 1) {
        const problem = validateValue(root, schema.items, value[index], indexPath(path, index), depth + 1);
        if (problem) return problem;
      }
    }
  }

  if (isObject(value)) {
    const properties = isObject(schema.properties) ? schema.properties : undefined;
    if (Array.isArray(schema.required)) {
      for (const name of schema.required) {
        if (typeof name === "string" && !Object.prototype.hasOwnProperty.call(value, name)) {
          return `Missing required argument "${childPath(path, name)}".`;
        }
      }
    }
    if (properties) {
      for (const [name, member] of Object.entries(value)) {
        if (Object.prototype.hasOwnProperty.call(properties, name)) {
          const problem = validateValue(root, properties[name], member, childPath(path, name), depth + 1);
          if (problem) return problem;
        }
      }
    }
    const minProperties = limitOf(schema, "minProperties");
    if (minProperties !== null && Object.keys(value).length < minProperties) {
      return `${argumentLabel(path)} must name at least ${minProperties} fields.`;
    }
    const maxProperties = limitOf(schema, "maxProperties");
    if (maxProperties !== null && Object.keys(value).length > maxProperties) {
      return `${argumentLabel(path)} must name at most ${maxProperties} fields.`;
    }
    // Without a pattern matcher, `additionalProperties: false` beside
    // `patternProperties` cannot tell a pattern-allowed name from an extra one,
    // so that is left to the server rather than guessed at.
    if (schema.patternProperties === undefined) {
      if (schema.additionalProperties === false) {
        for (const name of Object.keys(value)) {
          const known = Boolean(properties && Object.prototype.hasOwnProperty.call(properties, name));
          if (!known) return `Argument "${childPath(path, name)}" is not declared by the tool.`;
        }
      } else if (isObject(schema.additionalProperties)) {
        for (const [name, member] of Object.entries(value)) {
          const known = Boolean(properties && Object.prototype.hasOwnProperty.call(properties, name));
          if (known) continue;
          const problem = validateValue(root, schema.additionalProperties, member, childPath(path, name), depth + 1);
          if (problem) return problem;
        }
      }
    }
  }
  return null;
}

/**
 * Check a `tools/call` argument object against the tool's own `inputSchema`.
 * Null means accepted; a string is the reason, ready to show the model.
 */
export function validateArguments(schema: unknown, args: unknown): string | null {
  if (!isObject(args)) return "Tool arguments must be a JSON object.";
  return validateValue(schema, schema, args, "", 0);
}
