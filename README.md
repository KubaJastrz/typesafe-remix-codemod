# type-safe remix codemod

demo: https://x.com/kuba_jastrz/status/1798783656305025372

I don't know how to package this yet, but you can run it locally:

```bash
git clone https://github.com/KubaJastrz/typesafe-remix-codemod
cd typesafe-remix-codemod
cargo run ./remix-app  # or path to any other remix app
```

todo:
- [x] indent the result
- [ ] remove those ugly empty lines
- [ ] add import for `defineRoutes` at the top of the file
- [ ] read the `app` dir from config somehow, right now it is hardcoded
- [ ] read and inject `params` array (from the filepath? from the sourcecode?)
- [ ] remove useLoaderData altogether https://x.com/pcattori/status/1798355710968844784
- [ ] remove `LoaderFunctionArgs` from `loader({ params }: LoaderFunctionArgs)`
- [ ] support `clientLoader.hydrate = true`
