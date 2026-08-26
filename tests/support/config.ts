export const loginURL = process.env.LOGIN_URL ?? 'https://admin-test.noscura.net/login';
export const newPatientURL =
  process.env.NEW_PATIENT_URL ?? 'https://admin-test.noscura.net/newHomeAdmin?selectedOption=1';
export const loginEmail = process.env.LOGIN_EMAIL;
export const loginPassword = process.env.LOGIN_PASSWORD;
export const runLoginSmoke = process.env.RUN_NOSCURA_LOGIN_SMOKE === '1';
