const {parse} = require('@swc/core');
const {ConstantVisitor} = require('./const-visitor');

/** @type {(code: string) => Promise<Set<import('./const-visitor').ConstantExport>>} */
async function findConstantExports(code) {
  const script = await parse(code, {syntax: 'typescript', tsx: true});
  const visitor = new ConstantVisitor(code);
  visitor.visitProgram(script);
  return visitor.constantExports;
}

module.exports = {
  findConstantExports,
};
