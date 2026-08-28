# Demo tape

Record the README GIF. It catalogs two project skills for a review
prompt, then loads `review-pr`.

```bash
bash demo/setup.sh
HOME=/tmp/cb-demo-home PATH="$(pwd)/target/debug:$PATH" vhs /tmp/craftbag-demo.tape
```

The first VHS run after install may fail with `ERR_CONNECTION_REFUSED`.
Retry the same `vhs` command.

The recorded GIF is `demo/demo.gif` on the root README.
