import * as React from "react";

function Input(props: React.ComponentProps<"input">): React.ReactElement {
  return <input {...props} />;
}

export { Input };
