// Where the e2e scripts look for the server unless they are given a URL. PORT is the variable
// `cargo run` and `npm run dev` read, so one setting covers all three.
export const defaultUrl = `http://localhost:${process.env.PORT ?? 8800}/?debug`;
