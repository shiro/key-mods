// This example shows how to react to active window changes and query window information.

// query the active window class
print("the active window class is: " + active_window_class());

// register a callback that will be called whenever the active window changes
// mappings do not get reverted automatically if the window changes again, this needs to be done explicitly
on_window_change(||{
  if(active_window_class() == "firefox"){
    // map 'a' to 'b'
    a::b;
  }else if(active_window_class() == "Thunderbird"){
    // map 'a' to 'c'
    a::c;
  }else{
    // map 'a' back to 'a' since it might have been remapped
    a::a;
  }
});
