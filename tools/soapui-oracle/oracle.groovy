// Writes SoapUI's sample request for every operation of a WSDL to <wsdl dir>/expected/<Binding>__<Operation>.xml
import com.eviware.soapui.SoapUI
import com.eviware.soapui.StandaloneSoapUICore
import com.eviware.soapui.impl.wsdl.WsdlProject
import com.eviware.soapui.impl.wsdl.support.wsdl.WsdlImporter

def wsdl = new File(args[0]).canonicalFile
def out = new File(wsdl.parentFile, "expected")
out.mkdirs()
SoapUI.setSoapUICore(new StandaloneSoapUICore(true))
def project = new WsdlProject()
WsdlImporter.importWsdl(project, wsdl.toURI().toString()).each { iface ->
    iface.operationList.each { op ->
        new File(out, "${iface.name}__${op.name}.xml").setText(op.createRequest(true), "UTF-8")
    }
}
System.exit(0)
